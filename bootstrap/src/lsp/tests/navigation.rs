use super::*;

fn requirement() -> Requirement {
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
    }
}

// The card: the requirement linked to its docsgen heading, the code linked at the
// line each site relocates to, and the test linked at its name.
// Mirrors docs/frontends/lsp.md#capabilities.
#[test]
fn requirement_card_links_the_requirement_code_and_test() {
    let tmp = std::env::temp_dir().join(format!("jazyk-lsp-card-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&tmp).ok();
    let root = tmp.join("proj");
    let out = root.join(".jazyk");
    let deliv = tmp.join("product");
    crate::vfs::create_dir_all(deliv.join("src")).unwrap();
    crate::vfs::create_dir_all(deliv.join("tests")).unwrap();
    crate::vfs::create_dir_all(crate::layout::docsgen_dir(&out)).unwrap();
    // The site anchored at line 1; an inserted header moved it to line 3.
    crate::vfs::write(
        deliv.join("src/cart.rs"),
        "// header\n\nfn hold(i: Item) {}\n",
    )
    .unwrap();
    crate::vfs::write(
        deliv.join("tests/cart.rs"),
        "#[test]\nfn req_shop_1_abcd() {}\n",
    )
    .unwrap();
    crate::vfs::write(
        crate::layout::docsgen_dir(&out).join("cart.md"),
        "# Cart\n\n### `req:shop-1`\n",
    )
    .unwrap();

    let mut ledger = Ledger::default();
    ledger.requirements.insert(
        "req:shop-1".into(),
        ReqRow {
            tests: std::collections::BTreeMap::new(),
            entity: "ent:cart".into(),
            files: vec!["src/cart.rs".into(), "src/checkout.rs".into()],
            sites: vec![Site {
                file: "src/cart.rs".into(),
                line: 1,
                head: "fn hold(i: Item) {}".into(),
            }],
            test: TestRef {
                kind: "programmatic".into(),
                label: "unit".into(),
                artifact: "tests/cart.rs".into(),
                name: "req_shop_1_abcd".into(),
                run: "cargo test req_shop_1_abcd".into(),
                cwd: ".".into(),
            },
            hashes: RowHashes::default(),
            verdict: "none".into(),
            last_run: None,
            exit_code: None,
            evidence: None,
        },
    );
    ledger.save(&out).unwrap();

    let r = requirement();
    let mut store = Store {
        out: out.clone(),
        ..Default::default()
    };
    store
        .graph
        .requirements
        .insert("req:shop-1".into(), r.clone());
    let fingerprint = store_fingerprint(&root, &out);
    let lsp = Lsp {
        root,
        out,
        store,
        generation: 0,
        fingerprint,
        gen: GenSettings {
            configured: true,
            deliverable: deliv,
            worker: "agentic".into(),
            code: Vec::new(),
        },
        overlay: HashMap::new(),
        next_srv_id: 1,
        compiler: Compiler::Stdio {
            opts: Default::default(),
            running: None,
        },
    };
    let card = lsp.requirement_card("req:shop-1", &r);

    assert!(card.contains("**`req:shop-1`**"), "{}", card);
    assert!(card.contains("The Cart shall hold items."), "{}", card);
    // The requirement link: the docsgen heading, with the id for clients that route
    // to the node itself.
    assert!(
        card.contains("docsgen/cart.md?req=req:shop-1#L3"),
        "{}",
        card
    );
    // The code: the site relocated from line 1 to line 3, and marked moved. The
    // manifest file with no site links to the file itself.
    assert!(card.contains("[`src/cart.rs:3`]"), "{}", card);
    assert!(card.contains("#L3) · moved"), "{}", card);
    assert!(card.contains("[`src/checkout.rs`]"), "{}", card);
    // The test: the artifact at the line its name sits on, and the derived status
    // (the row's requirement hash is empty, so the statement moved under it).
    assert!(card.contains("[`tests/cart.rs:2`]"), "{}", card);
    assert!(card.contains("`req_shop_1_abcd`"), "{}", card);
    assert!(
        card.contains("↻ stale-requirement · requirement-changed"),
        "{}",
        card
    );
    assert!(
        card.contains("run `cargo test req_shop_1_abcd`"),
        "{}",
        card
    );
    crate::vfs::remove_dir_all(&tmp).ok();
}

// No ledger row: the requirement still shows, the other two parts say so.
#[test]
fn requirement_card_without_a_ledger_row() {
    let tmp =
        std::env::temp_dir().join(format!("jazyk-lsp-card-none-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&tmp).ok();
    let out = tmp.join(".jazyk");
    crate::vfs::create_dir_all(&out).unwrap();
    let r = requirement();
    let mut store = Store {
        out: out.clone(),
        ..Default::default()
    };
    store
        .graph
        .requirements
        .insert("req:shop-1".into(), r.clone());
    let fingerprint = store_fingerprint(&tmp, &out);
    let lsp = Lsp {
        root: tmp.clone(),
        out,
        store,
        generation: 0,
        fingerprint,
        gen: GenSettings {
            configured: true,
            deliverable: tmp.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        },
        overlay: HashMap::new(),
        next_srv_id: 1,
        compiler: Compiler::Stdio {
            opts: Default::default(),
            running: None,
        },
    };
    let card = lsp.requirement_card("req:shop-1", &r);
    assert!(card.contains("– not built"), "{}", card);
    assert!(card.contains("**code** · not generated"), "{}", card);
    assert!(card.contains("**test** · not generated"), "{}", card);
    crate::vfs::remove_dir_all(&tmp).ok();
}

fn write_svg(out: &std::path::Path, kind: &str, slug: &str) {
    let dir = crate::layout::diagrams_dir(out).join(kind);
    crate::vfs::create_dir_all(&dir).unwrap();
    crate::vfs::write(dir.join(format!("{}.svg", slug)), "<svg/>").unwrap();
}

// Hover on an entity with a level: the header, `Sits in`, the in-context image
// of the parent's level view captioned with its id, the `Inside` link with the
// child count (never a second image), and the line of links to the card, the
// requirements document, and the parent's level page.
// Mirrors docs/frontends/lsp.md#capabilities.
#[test]
fn entity_hover_renders_the_card_in_short_with_its_level() {
    let tmp = std::env::temp_dir().join(format!("jazyk-lsp-walk-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&tmp).ok();
    let out = tmp.join(".jazyk");
    crate::vfs::create_dir_all(&out).unwrap();
    write_svg(&out, "component", "shop");
    write_svg(&out, "component", "order-service");
    let lsp = lsp_at(tmp.clone(), out.clone(), shop_at(&out));
    let card = lsp.entity_card_short("ent:order-service").unwrap();
    let o = path_to_uri(&out);
    let blocks: Vec<&str> = card.split("\n\n").collect();
    assert_eq!(
        blocks,
        ["**Order Service** «service» · Takes and fulfils orders.".to_string(),
            "Sits in: Public › Shop › **Order Service**".to_string(),
            format!("![view:component/shop]({}/derived/diagrams/component/shop.svg)", o),
            format!(
                "Inside: [view:component/order-service]({}/derived/docsgen/diagrams/component/order-service.md) (4 children)",
                o
            ),
            format!(
                "[card]({o}/derived/docsgen/entities/order-service.md) · [requirements (2)]({o}/derived/docsgen/order-service.md) · [level: Shop]({o}/derived/docsgen/levels/shop.md)",
                o = o
            )]
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
    );
    // The inside view is rendered too, and still only the context is an image.
    assert_eq!(card.matches("![").count(), 1);
    // A parentless entity sits in the scope root: its level page is the scope's.
    let root = lsp.entity_card_short("ent:shop").unwrap();
    assert!(root.contains("Sits in: Public › **Shop**"), "{}", root);
    assert!(
        root.contains(&format!(
            "[level: Public]({}/derived/docsgen/levels/scope-public.md)",
            o
        )),
        "{}",
        root
    );
    assert!(lsp.entity_card_short("ent:nobody").is_none());
    crate::vfs::remove_dir_all(&tmp).ok();
}

// A leaf: no `Inside` line, its context is its parent's level, and when that
// rendering is missing the caption stands alone linked to the diagram page.
// Mirrors docs/frontends/lsp.md#capabilities.
#[test]
fn entity_hover_on_a_leaf_has_no_inside_and_survives_a_missing_rendering() {
    let tmp = std::env::temp_dir().join(format!("jazyk-lsp-leaf-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&tmp).ok();
    let out = tmp.join(".jazyk");
    crate::vfs::create_dir_all(&out).unwrap();
    write_svg(&out, "component", "order-service");
    let lsp = lsp_at(tmp.clone(), out.clone(), shop_at(&out));
    let o = path_to_uri(&out);
    let card = lsp.entity_card_short("ent:order-item").unwrap();
    assert!(card.starts_with("**Order Item**\n\n"), "{}", card);
    assert!(
        card.contains("Sits in: Public › Shop › Order Service › **Order Item**"),
        "{}",
        card
    );
    assert!(
        card.contains(&format!(
            "![view:component/order-service]({}/derived/diagrams/component/order-service.svg)",
            o
        )),
        "{}",
        card
    );
    assert!(!card.contains("Inside:"), "{}", card);
    assert!(
        card.contains(&format!(
            "[level: Order Service]({}/derived/docsgen/levels/order-service.md)",
            o
        )),
        "{}",
        card
    );
    crate::vfs::remove_file(
        crate::layout::derived_dir(&out).join("diagrams/component/order-service.svg"),
    )
    .unwrap();
    let card = lsp.entity_card_short("ent:order-item").unwrap();
    assert!(!card.contains("!["), "{}", card);
    assert!(
        card.contains(&format!(
            "In context: [view:component/order-service]({}/derived/docsgen/diagrams/component/order-service.md) (not rendered)",
            o
        )),
        "{}",
        card
    );
    crate::vfs::remove_dir_all(&tmp).ok();
}

// A derived grouping says so and names its pending ratification proposal.
// Mirrors docs/frontends/lsp.md#capabilities.
#[test]
fn entity_hover_on_a_derived_grouping_names_its_proposal() {
    use crate::model::{Diagnostic, Entity, Provenance};
    let tmp = std::env::temp_dir().join(format!("jazyk-lsp-derived-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&tmp).ok();
    let out = tmp.join(".jazyk");
    crate::vfs::create_dir_all(&out).unwrap();
    let mut s = crate::derive::tests::showcase_store();
    s.out = out.clone();
    s.graph.entities.insert(
        "ent:promotions".into(),
        Entity {
            name: "Promotions".into(),
            parent: Some("ent:shop".into()),
            provenance: Some(Provenance::Derived {
                from: vec!["ent:promo".into(), "ent:coupon".into()],
                reasoning: "both discount an order".into(),
            }),
            ..Default::default()
        },
    );
    for (id, name) in [("ent:promo", "Promo"), ("ent:coupon", "Coupon")] {
        s.graph.entities.insert(
            id.into(),
            Entity {
                name: name.into(),
                parent: Some("ent:promotions".into()),
                ..Default::default()
            },
        );
    }
    s.graph.diagnostics.insert(
        "diag:ratification-pending-1".into(),
        Diagnostic {
            goal: None,
            rule: "ratification-pending".into(),
            severity: "info".into(),
            subjects: vec!["ent:promotions".into()],
            message: "Promotions groups Promo and Coupon".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        },
    );
    let mut batch = crate::store::RecordBatch::new(1);
    crate::derive::recompute(&mut s, "g1", &mut batch);
    let lsp = lsp_at(tmp.clone(), out.clone(), s);
    let card = lsp.entity_card_short("ent:promotions").unwrap();
    assert!(
        card.ends_with("derived · proposal pending: `diag:ratification-pending-1`"),
        "{}",
        card
    );
    assert!(card.contains("(2 children)"), "{}", card);
    // A quoted entity carries no such line.
    let plain = lsp.entity_card_short("ent:order").unwrap();
    assert!(!plain.contains("proposal"), "{}", plain);
    crate::vfs::remove_dir_all(&tmp).ok();
}

// Over the wire: the capability is advertised, hover carries the short card, and
// type definition lands on the entity's card at line 0 while definition stays the
// defining mention. Mirrors docs/frontends/lsp.md#capabilities.
#[test]
fn type_definition_opens_the_card() {
    let tmp = std::env::temp_dir().join(format!("jazyk-lsp-typedef-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&tmp).ok();
    let out = tmp.join(".jazyk");
    crate::vfs::create_dir_all(&out).unwrap();
    let mut lsp = lsp_at(tmp.clone(), out.clone(), shop_at(&out));
    let mut wire: Vec<u8> = Vec::new();
    lsp.handle(
        json!({"id": 1, "method": "initialize", "params": {}}),
        &mut wire,
    );
    let init = String::from_utf8_lossy(&wire).to_string();
    assert!(init.contains("\"typeDefinitionProvider\":true"), "{}", init);

    let uri = path_to_uri(&tmp.join("shop.md"));
    let mut wire: Vec<u8> = Vec::new();
    lsp.handle(
        json!({"method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri,
            "text": "The Order Service takes orders.\n"}}}),
        &mut wire,
    );
    let at = json!({"textDocument": {"uri": uri}, "position": {"line": 0, "character": 6}});
    let mut wire: Vec<u8> = Vec::new();
    lsp.handle(
        json!({"id": 2, "method": "textDocument/typeDefinition", "params": at}),
        &mut wire,
    );
    let td = String::from_utf8_lossy(&wire).to_string();
    assert!(
        td.contains(&format!(
            "\"uri\":\"{}/derived/docsgen/entities/order-service.md\"",
            path_to_uri(&out)
        )),
        "{}",
        td
    );
    assert!(
        td.contains("\"start\":{\"line\":0,\"character\":0}"),
        "{}",
        td
    );

    let mut wire: Vec<u8> = Vec::new();
    lsp.handle(
        json!({"id": 3, "method": "textDocument/hover", "params": at}),
        &mut wire,
    );
    let hover = String::from_utf8_lossy(&wire).to_string();
    assert!(
        hover.contains("Sits in: Public › Shop › **Order Service**"),
        "{}",
        hover
    );
    assert!(
        hover.contains("docsgen/entities/order-service.md"),
        "{}",
        hover
    );

    // Nothing under the cursor: no type definition.
    let mut wire: Vec<u8> = Vec::new();
    lsp.handle(
        json!({"id": 4, "method": "textDocument/typeDefinition", "params": {
            "textDocument": {"uri": uri}, "position": {"line": 0, "character": 25}}}),
        &mut wire,
    );
    assert!(
        String::from_utf8_lossy(&wire).contains("\"result\":null"),
        "{}",
        String::from_utf8_lossy(&wire)
    );
    crate::vfs::remove_dir_all(&tmp).ok();
}

// The requirement card says the statement and embeds the smallest flow view
// listing the requirement. Mirrors docs/frontends/lsp.md#capabilities.
#[test]
fn requirement_card_embeds_the_smallest_flow_view_and_statement() {
    use crate::model::View;
    let tmp = std::env::temp_dir().join(format!("jazyk-lsp-flow-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&tmp).ok();
    let out = tmp.join(".jazyk");
    crate::vfs::create_dir_all(&out).unwrap();
    write_svg(&out, "usecase", "all");
    write_svg(&out, "sequence", "one");

    let r = requirement();
    let mut store = Store {
        out: out.clone(),
        ..Default::default()
    };
    store
        .graph
        .requirements
        .insert("req:shop-1".into(), r.clone());
    let view = |kind: &str, title: &str, members: &[&str]| View {
        kind: kind.into(),
        title: title.into(),
        members: members.iter().map(|m| m.to_string()).collect(),
        ..Default::default()
    };
    store.graph.views.insert(
        "view:usecase/all".into(),
        view("use-case", "All", &["req:shop-1", "req:shop-2"]),
    );
    store.graph.views.insert(
        "view:sequence/one".into(),
        view("sequence", "One", &["req:shop-1"]),
    );
    let lsp = lsp_at(tmp.clone(), out.clone(), store);
    let card = lsp.requirement_card("req:shop-1", &r);
    assert!(card.contains("The Cart shall hold items."), "{}", card);
    assert!(card.contains("![One (sequence)]"), "{}", card);
    assert!(card.contains("diagrams/sequence/one.svg"), "{}", card);
    crate::vfs::remove_dir_all(&tmp).ok();
}

// The five reader-facing words, first match wins.
// Mirrors docs/consumers/gen.md#status-words.
#[test]
fn status_words_map_the_derived_statuses() {
    use crate::verify::status_word as w;
    assert_eq!(w("verified", "passed", true), "question");
    assert_eq!(w("failing", "failed", false), "failing");
    assert_eq!(w("partial", "criteria-failing", false), "failing");
    assert_eq!(w("stale-code", "code-changed", false), "stale");
    assert_eq!(w("missing", "artifact-gone", false), "stale");
    assert_eq!(w("verified", "passed", false), "verified");
    for (s, r) in [
        ("missing", "not-generated"),
        ("unimplemented", "unimplemented"),
        ("unverified", "never-run"),
    ] {
        assert_eq!(w(s, r, false), "not built");
    }
}
