use super::*;

// A built medium makes the build an obligation: a manifest without one is
// rejected, and a recorded one always lists the decided artifact.
// Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
// A reply that writes its module and the entry in one answer has written both.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
#[test]
fn a_reply_can_carry_more_than_one_file() {
    let one = parse_file_replies("FILE: src/a.py\nprint(1)\n").unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].0, "src/a.py");
    assert_eq!(one[0].1.trim(), "print(1)");

    // A fence around the body is the model's markdown habit, not content.
    let fenced = parse_file_replies("FILE: src/a.py\n```python\nprint(1)\n```\n").unwrap();
    assert_eq!(fenced[0].1.trim(), "print(1)");

    let two = parse_file_replies("FILE: src/a.py\nprint(1)\n\nFILE: build.py\nimport a\na.go()\n")
        .unwrap();
    assert_eq!(two.len(), 2);
    assert_eq!(two[0].0, "src/a.py");
    assert_eq!(two[0].1.trim(), "print(1)");
    assert_eq!(two[1].0, "build.py");
    assert_eq!(two[1].1.trim(), "import a\na.go()");

    // An escaping path is dropped, not written.
    let bad = parse_file_replies("FILE: src/a.py\nx\nFILE: ../outside.py\ny\n").unwrap();
    assert_eq!(bad.len(), 1);
    assert!(parse_file_replies("no file line here").is_err());
}

// Every path a manifest names is confined before any side effect: an absolute
// path or one climbing out with `..` is rejected naming it, and the rejected
// record strips no marker, writes no ledger, and removes nothing.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
#[test]
fn manifest_paths_are_confined_to_the_deliverable() {
    let out = std::env::temp_dir().join(format!("jazyk-gen-confine-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
    crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
    // A file beside the deliverable that a climbing path could reach.
    crate::vfs::write(
        out.join("secret.txt"),
        "// req:shop-1 hash:deadbeef\nkeep\n",
    )
    .unwrap();
    crate::vfs::write(
        gs.deliverable.join("src/cart.rs"),
        "// req:shop-1 hash:deadbeef\nfn a() {}\n",
    )
    .unwrap();
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
    for (bad, field) in [
        (
            json!({"files": ["src/cart.rs", "../secret.txt"], "tests": [row.clone()]}),
            "files",
        ),
        (
            json!({"files": ["/etc/passwd"], "tests": [row.clone()]}),
            "files",
        ),
        (
            json!({"files": ["src/cart.rs"], "supportFiles": ["../../x"], "tests": [row.clone()]}),
            "supportFiles",
        ),
        (
            json!({"files": ["src/cart.rs"], "tests": [{"requirement": "req:shop-1", "kind": "programmatic", "artifact": "../secret.txt", "name": name, "run": "x"}]}),
            "artifact",
        ),
        (
            json!({"files": ["src/cart.rs"], "tests": [row.clone()], "build": {"run": "make", "cwd": "..", "produces": []}}),
            "build.cwd",
        ),
    ] {
        let err = mark(&s, "ent:cart", None, &bad, &gs).unwrap_err();
        assert!(err.contains(field), "{}: {}", field, err);
        assert!(
            !Ledger::path(&out).vexists(),
            "a rejected record writes no ledger"
        );
    }
    // No side effect reached either file: the markers are still there.
    assert!(crate::vfs::read_to_string(out.join("secret.txt"))
        .unwrap()
        .contains("hash:"));
    assert!(
        crate::vfs::read_to_string(gs.deliverable.join("src/cart.rs"))
            .unwrap()
            .contains("hash:")
    );
    // The clean spellings are normalized on record.
    let ok = json!({"files": ["./src/cart.rs", "src//cart.rs"], "tests": [row]});
    mark(&s, "ent:cart", None, &ok, &gs).unwrap();
    assert_eq!(
        Ledger::load(&out).entities["cart"].files,
        vec!["src/cart.rs".to_string()]
    );
    assert_eq!(confine_rel("a\\b\\c.py").unwrap(), "a/b/c.py");
    assert!(confine_rel("C:/x").is_err());
    assert!(confine_rel(".").is_err());
    assert_eq!(confine_cwd("").unwrap(), ".");
    crate::vfs::remove_dir_all(&out).ok();
}

// A path that starts with the deliverable directory's own name restates the root
// while no directory of that name exists under it: it lands under the root and the
// reply says so. A deliverable that holds such a directory keeps the path.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
#[test]
fn a_path_restating_the_deliverable_name_lands_under_the_root() {
    let out = std::env::temp_dir().join(format!("jazyk-gen-unroot-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    crate::vfs::create_dir_all(gs.deliverable.join("src")).unwrap();
    assert_eq!(
        unroot(&gs.deliverable, "product/src/cart.rs").as_deref(),
        Some("src/cart.rs")
    );
    assert_eq!(
        unroot(&gs.deliverable, "./product/src/cart.rs").as_deref(),
        Some("src/cart.rs")
    );
    assert_eq!(unroot(&gs.deliverable, "product").as_deref(), Some(""));
    assert_eq!(unroot(&gs.deliverable, "src/cart.rs"), None);
    assert_eq!(unroot(&gs.deliverable, "products/cart.rs"), None);
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "pub struct Cart;\n").unwrap();
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
    let r = mark(
        &s,
        "ent:cart",
        None,
        &json!({
            "files": ["product/src/cart.rs"],
            "tests": [{"requirement": "req:shop-1", "kind": "llm", "label": "judged",
                       "artifact": "criteria/req-shop-1.md", "name": "req_shop_1", "run": "llm",
                       "files": ["product/src/cart.rs"]}],
        }),
        &gs,
    )
    .unwrap();
    assert!(
        r["pathNote"][0].as_str().unwrap().contains("`src/cart.rs`"),
        "{}",
        r
    );
    let ledger = Ledger::load(&out);
    assert_eq!(
        ledger.entities["cart"].files,
        vec!["src/cart.rs".to_string()]
    );
    assert_eq!(
        ledger.requirements["req:shop-1"].files,
        vec!["src/cart.rs".to_string()]
    );
    // A deliverable that really holds `product/` keeps the path as given.
    crate::vfs::create_dir_all(gs.deliverable.join("product")).unwrap();
    assert_eq!(unroot(&gs.deliverable, "product/src/cart.rs"), None);
    crate::vfs::remove_dir_all(&out).ok();
}

// Ownership is the component group's: two members of one group both write and
// record the same page, and neither is refused; an entity of another group is.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
#[test]
fn members_of_one_group_share_a_page_and_another_group_is_refused() {
    let out =
        std::env::temp_dir().join(format!("jazyk-gen-group-own-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut s, gs) = fixture(&out);
    let ent = |name: &str, parent: Option<&str>| Entity {
        name: name.into(),
        parent: parent.map(String::from),
        ..Default::default()
    };
    s.graph.entities.insert("ent:org".into(), ent("Org", None));
    s.graph
        .entities
        .insert("ent:operations".into(), ent("Operations", Some("ent:org")));
    s.graph.entities.insert(
        "ent:store-manager".into(),
        ent("Store Manager", Some("ent:operations")),
    );
    s.graph
        .entities
        .insert("ent:sales".into(), ent("Sales", Some("ent:org")));
    for (rid, stmt, e) in [
        ("req:d-1", "Operations moves shipments.", "ent:operations"),
        (
            "req:d-2",
            "Operations has Store Manager as a role.",
            "ent:store-manager",
        ),
        ("req:d-3", "Sales sells to wholesale accounts.", "ent:sales"),
    ] {
        s.graph.requirements.insert(
            rid.into(),
            Requirement {
                statement: stmt.into(),
                entities: vec![e.into()],
                ..Default::default()
            },
        );
    }
    crate::vfs::create_dir_all(gs.deliverable.clone()).unwrap();
    crate::vfs::create_dir_all(out.join("gen/criteria")).unwrap();
    crate::vfs::write(gs.deliverable.join("departments.md"), "# Departments\n").unwrap();
    let row = |rid: &str| {
        let f = format!("criteria/{}.md", rid.replace(':', "-"));
        crate::vfs::write(
            out.join("gen").join(&f),
            format!(
                "{}\nInspect the product. PASS when satisfied. FAIL otherwise.\n",
                criteria_front_matter(rid, None, &s.graph.requirements[rid].statement)
            ),
        )
        .unwrap();
        json!({"requirement": rid, "kind": "llm", "label": "judged", "artifact": f,
               "name": rid.replace([':', '-'], "_"), "run": "llm"})
    };
    assert!(file_owner(&s, &Ledger::load(&out), "ent:sales", "departments.md").is_none());
    mark(
        &s,
        "ent:operations",
        None,
        &json!({"files": ["departments.md"], "tests": [row("req:d-1")]}),
        &gs,
    )
    .unwrap();
    // The same group: shared, never refused.
    mark(
        &s,
        "ent:store-manager",
        None,
        &json!({"files": ["departments.md"], "tests": [row("req:d-2")]}),
        &gs,
    )
    .unwrap();
    let ledger = Ledger::load(&out);
    assert_eq!(
        ledger.entities["operations"].files,
        vec!["departments.md".to_string()]
    );
    assert_eq!(
        ledger.entities["store-manager"].files,
        vec!["departments.md".to_string()]
    );
    assert!(file_owner(&s, &ledger, "ent:store-manager", "departments.md").is_none());
    assert!(file_owner(&s, &ledger, "ent:operations", "departments.md").is_none());
    // Another group: refused, the owner named.
    assert_eq!(
        file_owner(&s, &ledger, "ent:sales", "departments.md").as_deref(),
        Some("operations")
    );
    let err = mark(
        &s,
        "ent:sales",
        None,
        &json!({"files": ["departments.md"], "tests": [row("req:d-3")]}),
        &gs,
    )
    .unwrap_err();
    assert!(
        err.contains("`departments.md` belongs to entity `operations`"),
        "{}",
        err
    );
    assert!(err.contains("component group"), "{}", err);
    // The system generates alone: the group's page is not its own either, and a
    // requirement target (a bind goal) owns nothing.
    assert!(file_owner(&s, &ledger, "ent:org", "departments.md").is_some());
    assert!(file_owner(&s, &ledger, "req:d-1", "departments.md").is_some());
    assert_eq!(
        shared_with(&s, "ent:operations"),
        vec!["ent:store-manager".to_string()]
    );
    assert!(shared_with(&s, "ent:org").is_empty());
    crate::vfs::remove_dir_all(&out).ok();
}

#[test]
fn scoped_worker_explicitly_promotes_and_extends_a_shared_written_page() {
    let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
    let root = Path::new("/project");
    crate::vfs::create_dir_all(root.join(".jazyk")).unwrap();
    crate::vfs::write(root.join(crate::document_sets::CONFIG), "version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Site'\ndocs = ['docs/**']\nsource = ['deliverable/**']\n").unwrap();
    let project = crate::document_sets::Workspace::load(root)
        .unwrap()
        .resolve(Some("ds_main"))
        .unwrap();
    let gs = GenSettings::resolve(&project);
    let mut store = Store {
        out: project.out.clone(),
        ..Default::default()
    };
    for (id, name, rid, statement) in [
        (
            "ent:footer",
            "Footer",
            "req:footer",
            "The page displays the footer.",
        ),
        (
            "ent:top-menu",
            "Top Menu",
            "req:top-menu",
            "The page displays the menu.",
        ),
    ] {
        store.graph.entities.insert(
            id.into(),
            Entity {
                name: name.into(),
                ..Default::default()
            },
        );
        store.graph.requirements.insert(
            rid.into(),
            Requirement {
                statement: statement.into(),
                entities: vec![id.into()],
                ..Default::default()
            },
        );
        store.graph.criteria.insert(
            format!("crit:{}", id.trim_start_matches("ent:")),
            Criterion {
                requirement: rid.into(),
                statement: statement.into(),
                ..Default::default()
            },
        );
    }
    store.save_for_test();
    let page = "deliverable/page.html";
    let exclusive = "deliverable/footer.css";
    let artifact = "deliverable/tests/site.py";
    crate::vfs::create_dir_all(root.join("deliverable/tests")).unwrap();
    crate::vfs::write(root.join(page), "<html><body>\n<!-- req:footer hash:12345678 -->\n<footer>Existing footer</footer>\n</body></html>\n").unwrap();
    crate::vfs::write(root.join(exclusive), "footer { color: navy; }\n").unwrap();
    crate::vfs::write(
        root.join(artifact),
        "def checks_footer():\n    pass\ndef checks_menu():\n    pass\n",
    )
    .unwrap();
    let mut ledger = Ledger::load(&store.out);
    ledger.medium = Some(Medium {
        form: "Static HTML page".into(),
        produced: "written".into(),
        toolchain: "HTML".into(),
        artifact: page.into(),
    });
    ledger.save(&store.out).unwrap();
    let footer_test = json!({"requirement":"req:footer","criterion":"crit:footer","kind":"programmatic",
        "artifact":artifact,"name":"checks_footer","run":"task:site","files":[page]});
    let mut footer_manifest = json!({"files":[page,exclusive],"tests":[footer_test]});
    mark(&store, "ent:footer", None, &footer_manifest, &gs).unwrap();
    let original = crate::vfs::read_to_string(root.join(page)).unwrap();
    let mut ledger = Ledger::load(&store.out);
    let footer = ledger.requirements.get_mut("req:footer").unwrap();
    footer.verdict = "pass".into();
    footer.evidence = Some("Prior fixture judgment of these exact source bytes.".into());
    for test in footer.tests.values_mut() {
        test.verdict = "pass".into();
        test.evidence = footer.evidence.clone();
    }
    ledger.save(&store.out).unwrap();
    let prior_row = serde_json::to_value(&ledger.requirements["req:footer"]).unwrap();
    assert_eq!(
        crate::verify::status_of(
            &store,
            "req:footer",
            &ledger.requirements["req:footer"],
            &gs
        )
        .0,
        "verified"
    );
    assert!(shared_with(&store, "ent:top-menu").is_empty());

    // This is the same target fence as a Top Menu-only in-process worker.
    let server = crate::mcp::McpServer::with_bridge(
        project.clone(),
        store.out.clone(),
        vec!["generate".into()],
        true,
        crate::mcp::BridgeFlags {
            ephemeral: true,
            serve_files: true,
            only: Some("g:generate:ent:top-menu".into()),
            build_token: Some("shared-page-fixture".into()),
            ..Default::default()
        },
    );
    let call = |name: &str, args: Value| {
        server
            .handle("tools/call", &json!({"name":name,"arguments":args}))
            .unwrap()
    };
    let successful = |reply: Value| -> Value {
        assert!(!reply["isError"].as_bool().unwrap_or(false), "{reply}");
        serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let refused = call(
        "write_text_file",
        json!({"path":page,"content":"must not replace footer"}),
    );
    assert!(refused.to_string().contains("file-owned"), "{refused}");
    assert_eq!(
        crate::vfs::read_to_string(root.join(page)).unwrap(),
        original
    );

    // Reopen the recorded owner, preserve its bytes and complete manifest,
    // and explicitly add the real shared composition page to support.
    let owner = successful(call("begin_generation", json!({"entity":"ent:footer"})));
    footer_manifest["supportFiles"] = json!([page]);
    successful(call(
        "record_generation",
        json!({"entity":"ent:footer",
        "factHash":owner["factHash"],"manifest":footer_manifest}),
    ));
    let promoted = Ledger::load(&store.out);
    assert_eq!(promoted.support, [page]);
    assert_eq!(
        crate::vfs::read_to_string(root.join(page)).unwrap(),
        original
    );
    assert_eq!(
        serde_json::to_value(&promoted.requirements["req:footer"]).unwrap(),
        prior_row
    );
    assert!(file_owner(&store, &promoted, "ent:top-menu", page).is_none());

    let package = successful(call("begin_generation", json!({"entity":"ent:top-menu"})));
    assert_eq!(package["sharedWith"], json!([]));
    assert_eq!(package["supportFiles"], json!([page]));
    let next = original.replace(
        "<html><body>\n",
        "<html><body>\n<!-- req:top-menu hash:12345678 -->\n<nav>New menu</nav>\n",
    );
    successful(call("write_text_file", json!({"path":page,"content":next})));
    let exclusive_refusal = call(
        "write_text_file",
        json!({"path":exclusive,"content":"must not replace exclusive module"}),
    );
    assert!(
        exclusive_refusal.to_string().contains("file-owned"),
        "{exclusive_refusal}"
    );
    assert_eq!(
        crate::vfs::read_to_string(root.join(exclusive)).unwrap(),
        "footer { color: navy; }\n"
    );
    let outside = call(
        "write_text_file",
        json!({"path":"private/page.html","content":"outside Source"}),
    );
    assert!(
        outside.to_string().contains("source-selection"),
        "{outside}"
    );
    let bad_promotion = call(
        "record_generation",
        json!({"entity":"ent:top-menu",
        "factHash":package["factHash"],"manifest":{"files":[page],"supportFiles":["private/page.html"]}}),
    );
    assert!(
        bad_promotion.to_string().contains("source selection"),
        "{bad_promotion}"
    );
    assert_eq!(Ledger::load(&store.out).support, [page]);
    successful(call(
        "record_generation",
        json!({"entity":"ent:top-menu",
        "factHash":package["factHash"],"manifest":{"files":[page],"supportFiles":[page],"tests":[{
            "requirement":"req:top-menu","criterion":"crit:top-menu","kind":"programmatic","artifact":artifact,
            "name":"checks_menu","run":"task:site","files":[page]
        }]}}),
    ));

    let ledger = Ledger::load(&store.out);
    let content = crate::vfs::read_to_string(root.join(page)).unwrap();
    assert!(content.contains("<nav>New menu</nav>"));
    assert!(content.contains("<footer>Existing footer</footer>"));
    assert!(!content.contains("hash:"));
    assert_eq!(ledger.entities["footer"].files, [page, exclusive]);
    assert_eq!(ledger.entities["top-menu"].files, [page]);
    let menu = &ledger.requirements["req:top-menu"];
    assert_eq!(menu.files, [page]);
    assert_eq!(menu.sites[0].head, "<nav>New menu</nav>");
    assert_eq!(
        menu.hashes.files,
        hash_implementing(&store.out, &gs, &menu.files)
    );
    assert_eq!(
        crate::verify::status_of(&store, "req:top-menu", menu, &gs).0,
        "unverified"
    );
    let footer = &ledger.requirements["req:footer"];
    assert_eq!(footer.sites[0].head, "<footer>Existing footer</footer>");
    assert!(locate_head(&content, &footer.sites[0].head, footer.sites[0].line).is_some());
    assert_eq!(
        crate::verify::status_of(&store, "req:footer", footer, &gs).0,
        "stale-code"
    );
    assert!(file_owner(&store, &ledger, "ent:top-menu", exclusive).is_some());
    assert!(!root.join("private/page.html").vexists());
}
