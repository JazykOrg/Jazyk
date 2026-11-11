use super::*;

#[test]
fn get_entity_sees_the_sessions_staged_entities() {
    // canon_entity_id resolves staged ids, so the lookup must too: a session
    // reading an entity it just staged gets it back, not `unknown-id` with
    // that very id listed as a near miss.
    let mut t = session();
    t.dispatch(
        "upsert_entity",
        &json!({"name": "Gadget", "definition": "A gadget.",
                "mention": {"section": "/shop/cart", "quote": "A Gadget and a Line sit beside"}}),
    )
    .unwrap();
    let v = t
        .dispatch("get_entity", &json!({"id": "ent:gadget"}))
        .unwrap();
    assert_eq!(v["id"], "ent:gadget");
    assert_eq!(v["name"], "Gadget");
    // Read-your-writes: a staged update shows in the read, for a staged
    // create and for a committed entity alike.
    t.dispatch(
        "update_entity",
        &json!({"id": "ent:gadget", "definition": "A precise gadget.",
                "add_aliases": ["widget"]}),
    )
    .unwrap();
    let v = t
        .dispatch("get_entity", &json!({"id": "ent:gadget"}))
        .unwrap();
    assert_eq!(v["definition"], "A precise gadget.");
    assert_eq!(v["aliases"][0], "widget");
    t.dispatch(
        "update_entity",
        &json!({"id": "ent:customer", "definition": "a person who buys things"}),
    )
    .unwrap();
    let v = t
        .dispatch("get_entity", &json!({"id": "ent:customer"}))
        .unwrap();
    assert_eq!(v["definition"], "a person who buys things");
}

// A bare empty list reads as "ask again" and models loop on it; a miss must say what
// the graph holds and what to do instead. Mirrors docs/compiler/tools.md#read-tools.
#[test]
fn search_miss_names_the_graph_and_the_next_step() {
    let mut t = session();
    let r = t.dispatch("search", &json!({"query": "slides"})).unwrap();
    assert_eq!(r["hits"].as_array().unwrap().len(), 0);
    assert_eq!(r["entityCount"], 1);
    assert!(
        r["entities"][0].as_str().unwrap().contains("ent:customer"),
        "{}",
        r
    );
    assert!(
        r["next"].as_str().unwrap().contains("upsert_entity"),
        "{}",
        r
    );
    // A hit answers under the same key, so the caller reads one shape.
    // Several names in one call: one result per query, in order, misses included.
    // Mirrors docs/compiler/tools.md#read-tools.
    let many = t
        .dispatch(
            "search",
            &json!({"queries": ["Customer", "Zebra Crossing"]}),
        )
        .unwrap();
    let results = many["results"].as_array().unwrap();
    assert_eq!(results.len(), 2, "{}", many);
    assert_eq!(results[0]["query"], "Customer");
    assert!(
        !results[0]["hits"].as_array().unwrap().is_empty(),
        "{}",
        many
    );
    assert_eq!(results[1]["query"], "Zebra Crossing");
    assert!(
        results[1]["hits"].as_array().is_none_or(|h| h.is_empty()),
        "{}",
        many
    );
    // An empty item in the list is filler, dropped where the call enters
    // (docs/compiler/tools.md#validation-and-errors): the call answers the rest.
    let one = t
        .dispatch("search", &json!({"queries": ["Customer", ""]}))
        .unwrap();
    assert_eq!(
        one["results"].as_array().map(|a| a.len()),
        Some(1),
        "{}",
        one
    );
    let hit = t.dispatch("search", &json!({"query": "Customer"})).unwrap();
    assert_eq!(hit["hits"][0]["id"], "ent:customer");
}

// kind narrows the search: view matches on title, entity stays the default, and an
// unknown kind bounces. Mirrors docs/compiler/tools.md#read-tools.
#[test]
fn search_kind_view_matches_on_title() {
    let mut t = session();
    t.snapshot.graph.views.insert(
        "view:class/public".into(),
        View {
            kind: "class".into(),
            title: "Public surface".into(),
            ..Default::default()
        },
    );
    let r = t
        .dispatch("search", &json!({"query": "public", "kind": "view"}))
        .unwrap();
    assert_eq!(r["hits"][0]["id"], "view:class/public");
    assert_eq!(r["hits"][0]["name"], "Public surface");
    // The entity default never returns views.
    let r = t.dispatch("search", &json!({"query": "public"})).unwrap();
    assert_eq!(r["hits"].as_array().unwrap().len(), 0);
    // A view miss inventories views and guides reading, never entity creation.
    let r = t
        .dispatch("search", &json!({"query": "slides", "kind": "view"}))
        .unwrap();
    assert_eq!(r["hits"].as_array().unwrap().len(), 0);
    assert_eq!(r["viewCount"], 1);
    assert_eq!(r["views"][0], "view:class/public (Public surface)");
    assert!(r.get("entities").is_none());
    assert!(r.get("entityCount").is_none());
    assert!(r["next"].as_str().unwrap().contains("get_view"));
    assert!(!r["next"].as_str().unwrap().contains("upsert_entity"));
    // An unknown kind bounces.
    let err = t
        .dispatch("search", &json!({"query": "x", "kind": "diagram"}))
        .unwrap_err();
    assert_eq!(err.rule, "bad-args");
}

#[test]
fn view_search_miss_includes_staged_titles_once_and_bounds_inventory() {
    let mut t = session();
    t.snapshot.graph.views.clear();
    let empty = t
        .dispatch("search", &json!({"query": "requirements", "kind": "view"}))
        .unwrap();
    assert_eq!(empty["viewCount"], 0);
    assert_eq!(empty["views"], json!([]));
    assert!(empty["next"]
        .as_str()
        .unwrap()
        .contains("get_entity or load"));

    for i in 0..26 {
        t.snapshot.graph.views.insert(
            format!("view:class/{i:02}"),
            View {
                title: format!("Diagram {i}"),
                ..Default::default()
            },
        );
    }
    t.staged_views.insert(
        "view:class/00".into(),
        View {
            title: "Renamed diagram".into(),
            ..Default::default()
        },
    );
    t.staged_views.insert(
        "view:class/01-new".into(),
        View {
            title: "Staged diagram".into(),
            ..Default::default()
        },
    );
    let result = t
        .dispatch(
            "search",
            &json!({"queries": ["absent", "missing"], "kind": "view"}),
        )
        .unwrap();
    for row in result["results"].as_array().unwrap() {
        assert_eq!(row["viewCount"], 27);
        assert_eq!(row["views"].as_array().unwrap().len(), 25);
        assert_eq!(row["views"][0], "view:class/00 (Renamed diagram)");
        assert_eq!(row["views"][2], "view:class/01-new (Staged diagram)");
        assert!(row["next"].as_str().unwrap().contains("Refine the query"));
        assert!(!row.to_string().contains("ent:customer"));
    }
    let unlisted = t
        .dispatch("search", &json!({"query": "Diagram 25", "kind": "view"}))
        .unwrap();
    assert_eq!(unlisted["hits"][0]["id"], "view:class/25");
}

#[test]
fn a_bare_document_reference_reads_its_root_section() {
    let mut t = session();
    for r in ["shop.md", "shop.md#", "shop.md#/"] {
        let v = t.dispatch("read_section", &json!({"ref": r})).unwrap();
        assert_eq!(v["title"], "Shop", "{}", r);
        assert!(v["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c.as_str().unwrap().starts_with("shop.md#/shop/cart")));
    }
    let err = t
        .dispatch("read_section", &json!({"ref": "nope.md#"}))
        .unwrap_err();
    assert_eq!(err.rule, "bad-section");
}

// read_section and diagnostics replies are bounded and page on.
// Mirrors docs/compiler/tools.md#read-tools.
#[test]
fn read_section_and_diagnostics_replies_are_capped() {
    let mut t = session();
    let mut body = String::from("# Big\n");
    for i in 0..(READ_SECTION_LINES + 20) {
        body.push_str(&format!("Line {} of the long section.\n", i));
    }
    t.snapshot.docs.insert(
        "big.md".into(),
        DocRecord {
            content_hash: hash_hex(&body),
            sections: crate::md::parse_sections(&body),
            coverage: BTreeMap::new(),
        },
    );
    let v = t
        .dispatch("read_section", &json!({"ref": "big.md"}))
        .unwrap();
    assert_eq!(v["truncated"], true, "{}", v["nextLine"]);
    let next = v["nextLine"].as_u64().unwrap();
    assert!(v["raw"].as_str().unwrap().len() <= READ_SECTION_BYTES);
    let rest = t
        .dispatch("read_section", &json!({"ref": "big.md", "line": next}))
        .unwrap();
    assert!(rest.get("truncated").is_none(), "{}", rest["nextLine"]);
    assert!(rest["raw"]
        .as_str()
        .unwrap()
        .contains(&format!("Line {} of", READ_SECTION_LINES + 19)));
    for i in 0..(DIAGNOSTICS_PAGE + 7) {
        t.snapshot.graph.diagnostics.insert(
            format!("diag:x-{:03}", i),
            open_diag("ambiguity", &["ent:customer"]),
        );
    }
    let v = t.dispatch("diagnostics", &json!({})).unwrap();
    assert_eq!(v["diagnostics"].as_array().unwrap().len(), DIAGNOSTICS_PAGE);
    assert_eq!(v["count"], DIAGNOSTICS_PAGE + 7);
    assert_eq!(v["nextOffset"], DIAGNOSTICS_PAGE);
    let v = t
        .dispatch("diagnostics", &json!({"offset": DIAGNOSTICS_PAGE}))
        .unwrap();
    assert_eq!(v["diagnostics"].as_array().unwrap().len(), 7);
    assert!(v.get("truncated").is_none());
}

// A depth past u32 saturates: it loads deep, never wraps to a shallow load.
#[test]
fn a_huge_load_depth_saturates_instead_of_wrapping() {
    let mut t = session();
    t.snapshot
        .graph
        .entities
        .insert("ent:order".into(), plain("Order"));
    t.snapshot.graph.requirements.insert(
        "req:a".into(),
        quoted_req(
            "A Customer places an Order.",
            &["ent:customer", "ent:order"],
            "Customer",
        ),
    );
    t.dispatch(
        "load",
        &json!({"target": "ent:customer", "depth": 4294967296u64}),
    )
    .unwrap();
    assert!(
        t.loaded
            .items
            .iter()
            .any(|i| i.target == "ent:order" && i.what.starts_with("full")),
        "{:?}",
        t.loaded
            .items
            .iter()
            .map(|i| (&i.target, &i.what))
            .collect::<Vec<_>>()
    );
}

// Unfiltered, the build's bookkeeping is counted per rule; rule lists it.
// Mirrors docs/compiler/tools.md#read-tools.
#[test]
fn diagnostics_count_bookkeeping_unless_its_rule_is_asked() {
    let mut t = session();
    for (id, rule) in [
        ("diag:ratification-pending-1", "ratification-pending"),
        ("diag:ratification-pending-2", "ratification-pending"),
        ("diag:ambiguity-1", "ambiguity"),
    ] {
        t.snapshot.graph.diagnostics.insert(
            id.into(),
            Diagnostic {
                goal: None,
                rule: rule.into(),
                severity: "info".into(),
                lifecycle: "open".into(),
                subjects: vec!["ent:customer".into()],
                message: "m".into(),
                reasoning: None,
                triage: None,
                prompt: None,
                answer: None,
                created: None,
                updated: None,
            },
        );
    }
    let v = t.dispatch("diagnostics", &json!({})).unwrap();
    assert_eq!(v["count"], 1, "{}", v);
    assert_eq!(v["bookkeeping"]["ratification-pending"], 2, "{}", v);
    let v = t
        .dispatch("diagnostics", &json!({"rule": "ratification-pending"}))
        .unwrap();
    assert_eq!(v["count"], 2, "{}", v);
}

// Past the high-water mark, load and expand refuse naming candidates; reads
// still answer. Mirrors docs/compiler/context.md#policy.
#[test]
fn load_past_the_high_water_mark_names_unload_candidates() {
    let mut t = session();
    t.dispatch("load", &json!({"target": "ent:customer"}))
        .unwrap();
    t.loaded.high_water = 1;
    let err = t
        .dispatch("load", &json!({"target": "shop.md#/shop/cart"}))
        .unwrap_err();
    assert_eq!(err.rule, "context-full");
    assert!(err.message.contains("ent:customer"), "{}", err.message);
    assert!(
        t.dispatch("search", &json!({"query": "buy"})).is_ok(),
        "reads still answer past the mark"
    );
}

// get_view carries the relationships among its members, a hidden member's lifted
// to its nearest shown ancestor. Mirrors docs/compiler/tools.md#read-tools.
#[test]
fn get_view_lists_member_relationships_lifted() {
    let mut s = Store::default();
    for (id, name) in [
        ("ent:server", "Server"),
        ("ent:db", "Database"),
        ("ent:cache", "Cache"),
    ] {
        s.graph.entities.insert(id.into(), plain(name));
    }
    s.graph
        .entities
        .insert("ent:model".into(), under("Model", "ent:server"));
    let rel = |a: &str, b: &str, ty: &str| Relationship {
        members: vec![a.into(), b.into()],
        contributions: vec![Contribution {
            a: a.into(),
            b: b.into(),
            r#type: ty.into(),
            cardinality: Some("1".into()),
            requirements: vec!["req:x".into()],
        }],
    };
    s.graph
        .relationships
        .insert("rel:1".into(), rel("ent:model", "ent:db", "dependency"));
    s.graph.relationships.insert(
        "rel:2".into(),
        rel("ent:server", "ent:cache", "association"),
    );
    s.graph.views.insert(
        "view:class/backend".into(),
        View {
            kind: "class".into(),
            title: "Backend".into(),
            members: vec!["ent:server".into(), "ent:db".into(), "ent:cache".into()],
            ..Default::default()
        },
    );
    let mut t = ToolSession::new(s, WorkScope::serving("mcp-compile"), 64, 24_000);
    let r = t
        .dispatch("get_view", &json!({"id": "view:class/backend"}))
        .unwrap();
    assert_eq!(
        r["relationships"],
        json!([
            {"a": "ent:server", "b": "ent:cache", "type": "association", "cardinality": "1"},
            {"a": "ent:server", "b": "ent:db", "type": "dependency"},
        ]),
        "{}",
        r["relationships"]
    );
}

// get_view lists, for every member with a level view, the member and that view:
// one present in the snapshot, one the containment tree yields at commit, none
// for a member without a level. Mirrors docs/compiler/tools.md#read-tools.
#[test]
fn get_view_lists_the_members_level_views_as_children() {
    let mut s = Store::default();
    s.graph.entities.insert(
        "ent:backend".into(),
        Entity {
            name: "Backend".into(),
            stereotype: Some("system".into()),
            ..Default::default()
        },
    );
    for (id, name) in [
        ("ent:server", "Server"),
        ("ent:db", "Database"),
        ("ent:cache", "Cache"),
    ] {
        s.graph
            .entities
            .insert(id.into(), under(name, "ent:backend"));
    }
    for (id, name) in [("ent:model", "Model"), ("ent:controller", "Controller")] {
        s.graph
            .entities
            .insert(id.into(), under(name, "ent:server"));
    }
    for (id, name) in [
        ("ent:orders-table", "Orders Table"),
        ("ent:users-table", "Users Table"),
    ] {
        s.graph.entities.insert(id.into(), under(name, "ent:db"));
    }
    let view = |title: &str, kind: &str, members: &[&str]| View {
        kind: kind.into(),
        title: title.into(),
        members: members.iter().map(|m| m.to_string()).collect(),
        default: true,
        ..Default::default()
    };
    s.graph.views.insert(
        "view:component/backend".into(),
        view(
            "Backend",
            "component",
            &["ent:server", "ent:db", "ent:cache"],
        ),
    );
    s.graph.views.insert(
        "view:class/server".into(),
        view("Server", "class", &["ent:model", "ent:controller"]),
    );
    let mut t = ToolSession::new(s, WorkScope::serving("mcp-compile"), 64, 24_000);
    let r = t
        .dispatch("get_view", &json!({"id": "view:component/backend"}))
        .unwrap();
    assert_eq!(
        r["children"],
        json!([
            {"member": "ent:server", "view": "view:class/server"},
            {"member": "ent:db", "view": "view:class/db"},
        ])
    );
    // A flow view links through its participants.
    t.snapshot.graph.requirements.insert(
        "req:shop-1".into(),
        quoted_req(
            "The server reads the model.",
            &["ent:server", "ent:model"],
            "reads",
        ),
    );
    t.snapshot.graph.views.insert(
        "view:usecase/backend-server-shop".into(),
        view("Server: Shop", "use-case", &["req:shop-1"]),
    );
    let r = t
        .dispatch(
            "get_view",
            &json!({"id": "view:usecase/backend-server-shop"}),
        )
        .unwrap();
    assert_eq!(
        r["children"],
        json!([{"member": "ent:server", "view": "view:class/server"}])
    );
}
