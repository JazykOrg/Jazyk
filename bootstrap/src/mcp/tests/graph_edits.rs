use super::*;

fn fixture(tag: &str) -> (std::path::PathBuf, crate::project::Project, McpServer) {
    let dir = bare_dir(tag);
    crate::vfs::write(
        dir.join("docs/domain.md"),
        "# Domain\n\nA Cart holds items.\n",
    )
    .unwrap();
    crate::vfs::write(
        dir.join("docs/reader.md"),
        "# Reader\n\nThe Cart shows its items.\n",
    )
    .unwrap();
    let project = crate::document_sets::Workspace::load(&dir)
        .unwrap()
        .resolve(Some("ds_main"))
        .unwrap();
    let server = McpServer::with_bridge(
        project.clone(),
        project.out.clone(),
        vec!["chat".into()],
        false,
        BridgeFlags {
            ephemeral: true,
            ..Default::default()
        },
    );
    (dir, project, server)
}

fn call(server: &McpServer, name: &str, arguments: Value) -> Value {
    let reply = server
        .handle("tools/call", &json!({"name":name,"arguments":arguments}))
        .unwrap();
    let value: Value = serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_ne!(reply["isError"], true, "{name}: {value}");
    value
}

fn cart(server: &McpServer) {
    call(
        server,
        "upsert_entity",
        json!({"name":"Cart","mention":{"section":"docs/domain.md#/domain","quote":"A Cart holds items."}}),
    );
}

fn requirement(server: &McpServer) -> String {
    call(server, "upsert_requirement", json!({"statement":"The Cart shows its items.","entities":["ent:cart"],"section":"docs/reader.md#/reader","quote":"The Cart shows its items.","facets":[{"facet":"behavior","reasoning":"Displays the contents."}]}))["id"].as_str().unwrap().to_string()
}

#[test]
fn chat_authored_graph_families_work_without_build_goals_or_release() {
    let (dir, project, server) = fixture("chat-graph-families");
    let before = crate::control::Control::load(&project, &project.out);
    cart(&server);
    let rid = requirement(&server);
    let criterion = call(
        &server,
        "upsert_criterion",
        json!({"requirement":rid,"statement":"A Cart containing two items shows both items.","kind":"positive","provenance":{"derived":{"from":[rid],"reasoning":"A concrete case of displaying the cart's contents."}}}),
    );
    assert!(Store::load(&project.out)
        .graph
        .criteria
        .contains_key(criterion["id"].as_str().unwrap()));
    let view = call(
        &server,
        "upsert_view",
        json!({"kind":"class","title":"Cart details","members":["ent:cart"],"reasoning":"Inspect the Cart independently of a build."}),
    );
    call(
        &server,
        "update_view",
        json!({"id":view["id"],"title":"Cart overview"}),
    );
    call(
        &server,
        "set_coverage",
        json!({"section":"docs/reader.md#/reader","state":"covered"}),
    );
    let diagnostic = call(
        &server,
        "report_diagnostic",
        json!({"rule":"ambiguity","severity":"info","subjects":["ent:cart"],"message":"The item ordering is unspecified."}),
    );
    call(
        &server,
        "resolve_diagnostic",
        json!({"id":diagnostic["id"],"reason":"No ordering obligation is stated."}),
    );
    let store = Store::load(&project.out);
    assert_eq!(
        store.graph.views[view["id"].as_str().unwrap()].title,
        "Cart overview"
    );
    assert!(server.open.lock().unwrap().is_none());
    let after = crate::control::Control::load(&project, &project.out);
    assert_eq!(before.compile, after.compile);
    assert_eq!(before.generate, after.generate);
    assert!(!crate::board::Board::compute(&project, &project.out)
        .goals
        .is_empty());
    assert_eq!(store.status.spent.sessions, 0);
    for g in 1..=store.status.generation {
        assert!(
            crate::layout::journal_file(&project.out, g).exists(),
            "missing journal g{g}"
        );
    }
    crate::vfs::remove_dir_all(dir).unwrap();
}

#[test]
fn graph_transaction_rolls_back_validation_commit_and_document_sync_failures() {
    let (dir, project, server) = fixture("chat-graph-rollback");
    cart(&server);
    let rid = requirement(&server);
    let before = Store::load(&project.out);
    crate::vfs::write(dir.join("docs/new.md"), "# New\n\nAnother input.\n").unwrap();
    let edit = json!({"tool":"update_entity","arguments":{"id":"ent:cart","definition":"A changed definition."}});
    for last in [
        json!({"tool":"update_entity","arguments":{"id":"ent:missing","definition":"Must fail."}}),
        json!({"tool":"classify_input","arguments":{"path":"docs/reader.md","mode":"asset","sha256":crate::task_runtime::digest(b"# Reader\n\nThe Cart shows its items.\n"),"reason":"Reader-facing asset."}}),
        json!({"tool":"run_task","arguments":{}}),
        json!({"tool":"edit_graph","arguments":{"edits":[]}}),
        json!({"tool":"update_entity","arguments":{"setId":"foreign","id":"ent:cart"}}),
    ] {
        refusal(server.handle(
            "tools/call",
            &json!({"name":"edit_graph","arguments":{"edits":[edit,last]}}),
        ));
        let after = Store::load(&project.out);
        assert_eq!(after.status.generation, before.status.generation);
        assert_eq!(
            after.graph.entities["ent:cart"].definition,
            before.graph.entities["ent:cart"].definition
        );
        assert!(after.graph.requirements.contains_key(&rid));
        assert!(!after.docs.contains_key("docs/new.md"));
        assert!(!after.decisions.inputs.contains_key("docs/reader.md"));
        assert!(!crate::layout::journal_file(&project.out, before.status.generation + 1).exists());
    }
    let result = call(
        &server,
        "edit_graph",
        json!({"edits":[
            {"tool":"delete_requirement","arguments":{"id":rid,"reason":"This reader-facing content is an asset."}},
            {"tool":"classify_input","arguments":{"path":"docs/reader.md","mode":"asset","sha256":crate::task_runtime::digest(b"# Reader\n\nThe Cart shows its items.\n"),"reason":"Reader-facing asset."}}
        ]}),
    );
    assert_eq!(result["committed"], true);
    assert_eq!(result["results"].as_array().unwrap().len(), 2);
    let after = Store::load(&project.out);
    assert!(!after.graph.requirements.contains_key(&rid));
    assert_eq!(after.decisions.inputs["docs/reader.md"].mode, "asset");
    assert!(after.docs.contains_key("docs/new.md"));
    crate::vfs::remove_dir_all(dir).unwrap();
}

#[test]
fn graph_edits_record_model_judgment_and_explicit_human_decisions_separately() {
    let (dir, project, server) = fixture("chat-graph-provenance");
    cart(&server);
    let rid = requirement(&server);
    let base = json!({"id":rid,"field":"facets","value":[{"facet":"behavior","reasoning":"Displays the contents."}]});
    let generation = crate::store::read_generation(&project.out);
    refusal(server.handle("tools/call", &json!({"name":"edit_fact","arguments":base})));
    assert_eq!(crate::store::read_generation(&project.out), generation);
    let mut derived = base.clone();
    derived["provenance"] = json!({"derived":{"from":["ent:cart"],"reasoning":"Displaying contents is behavior of the Cart."}});
    let reply = call(&server, "edit_fact", derived.clone());
    assert_eq!(reply["path"], "derived");
    let store = Store::load(&project.out);
    assert!(matches!(
        store.graph.requirements[&rid].provenance,
        Some(crate::model::Provenance::Derived { .. })
    ));
    assert!(store.pending_proposals() > 0);
    derived["decision"] = json!("Treat this as a behavior.");
    refusal(server.handle(
        "tools/call",
        &json!({"name":"edit_fact","arguments":derived}),
    ));
    let mut human = base;
    human["decision"] = json!("Treat this as a behavior.");
    let reply = call(&server, "edit_fact", human);
    assert_eq!(reply["path"], "decree");
    assert!(
        matches!(&Store::load(&project.out).graph.requirements[&rid].provenance, Some(crate::model::Provenance::Decree {note: Some(note), ..}) if note == "Treat this as a behavior.")
    );
    crate::vfs::remove_dir_all(dir).unwrap();
}

#[test]
fn independent_transaction_invalidates_batch_and_survives_its_abandon() {
    let (dir, project, server) = fixture("chat-graph-independent");
    cart(&server);
    let mut control = crate::control::Control::load(&project, &project.out);
    control.compile = "auto".into();
    control.save(&project.out);
    call(&server, "begin_goals", json!({}));
    // Even an unrelated compiler goal does not hide the independent edit surface.
    server
        .open
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .session
        .scope
        .goals[0]
        .kind = "declare-edges".into();
    assert!(tool_names(&server).contains(&"edit_graph".into()));
    assert!(tool_names(&server).contains(&"update_entity".into()));
    refusal(server.handle("tools/call", &json!({"name":"update_entity","arguments":{"id":"ent:cart","definition":"Cannot borrow compiler scope."}})));
    let result = call(
        &server,
        "edit_graph",
        json!({"edits":[{"tool":"update_entity","arguments":{"id":"ent:cart","definition":"An independently edited cart."}}]}),
    );
    assert_eq!(result["batchStale"], true);
    assert!(server.open.lock().unwrap().as_ref().unwrap().invalidated);
    let checkpoint = server.checkpoint().unwrap();
    assert_eq!(checkpoint["open"]["invalidated"], true);
    let error = refusal(server.handle("tools/call", &json!({"name":"done","arguments":{}})));
    assert!(error.contains("batch-stale"), "{error}");
    call(
        &server,
        "abandon_goals",
        json!({"reason":"Reload the independent edit."}),
    );
    assert_eq!(
        Store::load(&project.out).graph.entities["ent:cart"]
            .definition
            .as_deref(),
        Some("An independently edited cart.")
    );
    crate::vfs::remove_dir_all(dir).unwrap();
}

#[test]
fn transaction_cannot_escape_read_only_or_scoped_workers_and_shares_mutation_budget() {
    let (dir, project, mut server) = fixture("chat-graph-boundaries");
    cart(&server);
    let edit = json!({"tool":"edit_fact","arguments":{"id":"ent:cart","field":"definition","value":"A revised cart.","decision":"Use this definition."}});
    let transaction = json!({"edits":[edit]});
    for flags in [
        BridgeFlags {
            reader: true,
            ..Default::default()
        },
        BridgeFlags {
            only: Some("b1-1".into()),
            ..Default::default()
        },
    ] {
        let restricted = McpServer::with_bridge(
            project.clone(),
            project.out.clone(),
            vec!["chat".into()],
            false,
            flags,
        );
        assert!(!tool_names(&restricted).contains(&"edit_graph".into()));
        refusal(restricted.handle(
            "tools/call",
            &json!({"name":"edit_graph","arguments":transaction}),
        ));
    }
    let generation = crate::store::read_generation(&project.out);
    server.mutation_limit = 1;
    let error = refusal(server.handle(
        "tools/call",
        &json!({"name":"edit_graph","arguments":transaction}),
    ));
    assert!(error.contains("mutation-budget"), "{error}");
    assert_eq!(crate::store::read_generation(&project.out), generation);
    assert!(
        Store::load(&project.out).graph.entities["ent:cart"]
            .definition
            .as_deref()
            != Some("A revised cart.")
    );
    crate::vfs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_separate_chat_cannot_be_overwritten_by_a_stale_compiler_worker() {
    let (dir, project, chat) = fixture("chat-graph-external");
    cart(&chat);
    let mut control = crate::control::Control::load(&project, &project.out);
    control.compile = "auto".into();
    control.save(&project.out);
    let worker = McpServer::with_bridge(
        project.clone(),
        project.out.clone(),
        vec!["compile".into()],
        true,
        BridgeFlags {
            ephemeral: true,
            ..Default::default()
        },
    );
    call(&worker, "begin_goals", json!({}));
    call(
        &chat,
        "update_entity",
        json!({"id":"ent:cart","definition":"Edited in another conversation."}),
    );
    let generation = crate::store::read_generation(&project.out);
    let error = refusal(worker.handle("tools/call", &json!({"name":"done","arguments":{}})));
    assert!(error.contains("batch-stale"), "{error}");
    worker.eof_finish();
    assert_eq!(crate::store::read_generation(&project.out), generation);
    assert_eq!(
        Store::load(&project.out).graph.entities["ent:cart"]
            .definition
            .as_deref(),
        Some("Edited in another conversation.")
    );
    assert!(crate::control::leases(&project.out).is_empty());
    crate::vfs::remove_dir_all(dir).unwrap();
}
