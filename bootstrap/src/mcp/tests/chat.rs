use super::*;

#[test]
fn chat_links_pending_criterion_to_existing_description_without_editing_prose() {
    let dir = bare_dir("criterion-source-link");
    let path = dir.join("docs/landing.md");
    let prose = "# Landing Page\n## Hero\n### Title\n`Jazyk` with pronunciation `/jazɪk/` (smaller font and greyed out).\n";
    crate::vfs::write(&path, prose).unwrap();
    let project = crate::project::Project::load(&dir);
    let mut store = Store::load(&project.out);
    let (parsed, _) = crate::reconcile::parse_all(&project);
    store.sync_docs(&parsed);
    store.graph.requirements.insert(
        "req:landing-1".into(),
        crate::model::Requirement {
            statement: "The hero title displays Jazyk and its pronunciation.".into(),
            source: Some(crate::model::SourceRef {
                doc: "docs/landing.md".into(),
                section: "/landing-page/hero/title".into(),
                quote: "`Jazyk` with pronunciation `/jazɪk/` (smaller font and greyed out).".into(),
            }),
            ..Default::default()
        },
    );
    let statement = "A visitor opening the Landing Page sees the hero title `Jazyk` with pronunciation `/jazɪk/` in a smaller, greyed-out font.";
    let report = store.apply(
        vec![crate::store::Op::CreateCriterion {
            id: "crit:landing-1".into(),
            criterion: crate::model::Criterion {
                requirement: "req:landing-1".into(),
                statement: statement.into(),
                kind: "positive".into(),
                provenance: Some(crate::model::Provenance::Derived {
                    from: vec!["req:landing-1".into()],
                    reasoning: "The appearance a visitor observes".into(),
                }),
                ..Default::default()
            },
        }],
        &crate::store::Commit::store("test"),
    );
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(store.pending_proposals(), 1);
    let cid = store.criteria_of("req:landing-1")[0].clone();
    let serving = McpServer::with_bridge(
        project.clone(),
        project.out.clone(),
        vec!["chat".into()],
        false,
        BridgeFlags {
            ephemeral: true,
            ..Default::default()
        },
    );
    for args in [
        json!({"id":cid,"section":"docs/landing.md#/landing-page/hero/title"}),
        json!({"id":cid,"section":"docs/landing.md#/landing-page/hero/title","quote":"An invented quotation."}),
    ] {
        let reply = serving
            .handle(
                "tools/call",
                &json!({"name":"update_criterion","arguments":args}),
            )
            .unwrap();
        assert_eq!(reply["isError"], true, "{reply}");
        assert_eq!(Store::load(&project.out).pending_proposals(), 1);
    }
    let reply = serving
        .handle(
            "tools/call",
            &json!({"name":"update_criterion","arguments":{
                "id":cid,"section":"docs/landing.md#/landing-page/hero/title",
                "quote":"`Jazyk` with pronunciation `/jazɪk/` (smaller font and greyed out)."
            }}),
        )
        .unwrap();
    assert_ne!(reply["isError"], true, "{reply}");
    let store = Store::load(&project.out);
    let c = &store.graph.criteria[&cid];
    assert_eq!(c.statement, statement);
    assert!(c.provenance.is_none());
    assert_eq!(c.source, store.graph.requirements["req:landing-1"].source);
    assert_eq!(store.pending_proposals(), 0);
    assert!(!store
        .status
        .has_change(crate::store::CHANGE_PROVENANCE_PENDING, &cid));
    assert_eq!(crate::vfs::read_to_string(&path).unwrap(), prose);
    crate::vfs::remove_dir_all(dir).ok();
}

#[test]
fn chat_criterion_edit_commits_without_goal_batch_and_validates_fields() {
    let dir = bare_dir("chat-criterion");
    let project = crate::project::Project::load(&dir);
    let mut store = Store::load(&project.out);
    store.graph.requirements.insert(
        "req:hosting-3".into(),
        crate::model::Requirement {
            statement: "The deployment runs on main.".into(),
            ..Default::default()
        },
    );
    store.graph.criteria.insert(
        "crit:hosting-11".into(),
        crate::model::Criterion {
            requirement: "req:hosting-3".into(),
            statement: "A push to master triggers deployment.".into(),
            kind: "positive".into(),
            ..Default::default()
        },
    );
    store.save_for_test();
    let serving = McpServer::with_bridge(
        project.clone(),
        project.out.clone(),
        vec!["chat".into()],
        false,
        BridgeFlags {
            ephemeral: true,
            ..Default::default()
        },
    );
    let reply = serving.handle("tools/call", &json!({"name":"update_criterion","arguments":{"id":"crit:hosting-11","statement":"A push to main triggers deployment."}})).unwrap();
    assert_ne!(reply["isError"], true, "{reply}");
    assert_eq!(
        Store::load(&project.out).graph.criteria["crit:hosting-11"].statement,
        "A push to main triggers deployment."
    );
    let bad = serving.handle("tools/call", &json!({"name":"update_criterion","arguments":{"id":"crit:hosting-11","kind":"invalid"}})).unwrap();
    assert_eq!(bad["isError"], true);
    assert_eq!(
        Store::load(&project.out).graph.criteria["crit:hosting-11"].kind,
        "positive"
    );
    let empty = serving
        .handle(
            "tools/call",
            &json!({"name":"update_criterion","arguments":{"id":"crit:hosting-11"}}),
        )
        .unwrap();
    assert_eq!(empty["isError"], true);
    let reader = McpServer::with_bridge(
        project.clone(),
        project.out.clone(),
        vec!["chat".into()],
        false,
        BridgeFlags {
            ephemeral: true,
            reader: true,
            ..Default::default()
        },
    );
    let refused = reader.handle("tools/call", &json!({"name":"update_criterion","arguments":{"id":"crit:hosting-11","statement":"must not land"}}));
    assert!(refused.is_err() || refused.unwrap()["isError"] == true);
    assert_eq!(
        Store::load(&project.out).graph.criteria["crit:hosting-11"].statement,
        "A push to main triggers deployment."
    );

    assert!(serving.open.lock().unwrap().is_none());
    crate::vfs::remove_dir_all(dir).ok();
}

#[test]
fn chat_prose_edit_works_before_compile_and_rejects_stale_or_unsafe_writes() {
    let dir = bare_dir("chat-uncompiled-prose");
    let path = dir.join("docs/hosting.md");
    crate::vfs::write(&path, "# Hosting\n\nCloudFront-S3.\n").unwrap();
    let project = crate::document_sets::Workspace::load(&dir)
        .unwrap()
        .files_project();
    let s = McpServer::with_bridge(
        project.clone(),
        project.out.clone(),
        vec!["chat".into()],
        false,
        BridgeFlags {
            ephemeral: true,
            ..Default::default()
        },
    );
    let call = |name: &str, args: Value| {
        s.handle("tools/call", &json!({"name":name,"arguments":args}))
            .unwrap()
    };
    let read = call("read_document", json!({"path":"docs/hosting.md"}));
    let read: Value = serde_json::from_str(read["content"][0]["text"].as_str().unwrap()).unwrap();
    let changed = "# Hosting\n\nUse the existing deployment task.\n";
    let written = call(
        "write_document",
        json!({"path":"docs/hosting.md","text":changed,"baseHash":read["hash"]}),
    );
    assert!(!written["isError"].as_bool().unwrap_or(false), "{written}");
    assert_eq!(crate::vfs::read_to_string(&path).unwrap(), changed);
    let stale = call(
        "write_document",
        json!({"path":"docs/hosting.md","text":"must not land","baseHash":read["hash"]}),
    );
    assert_eq!(stale["isError"], true);
    assert_eq!(crate::vfs::read_to_string(&path).unwrap(), changed);
    assert!(!refusal(s.handle(
        "tools/call",
        &json!({"name":"write_document","arguments":{"path":"../outside.md","text":"x"}})
    ))
    .is_empty());
    assert!(!refusal(s.handle(
        "tools/call",
        &json!({"name":"write_document","arguments":{"path":"docs/a.pdf","text":"x"}})
    ))
    .is_empty());
    assert!(Store::load(&dir.join(".jazyk/ds_main"))
        .graph
        .entities
        .is_empty());
    assert!(crate::control::Control::load(&project, &project.out)
        .released
        .compile
        .is_empty());
}

// Mirrors docs/frontends/mcp.md#watching-edits.
#[test]
fn recent_edits_reports_an_outside_edit_with_its_derived_goals() {
    let dir = bare_dir("recent-edits");
    let project = crate::document_sets::Workspace::load(&dir)
        .unwrap()
        .files_project();
    let s = McpServer::with_bridge(
        project.clone(),
        project.out.clone(),
        vec!["chat".into()],
        false,
        BridgeFlags::default(),
    );
    s.handle("initialize", &json!({"clientInfo":{"name":"test"}}))
        .unwrap();
    let listed = s.handle("tools/list", &json!({})).unwrap();
    assert!(
        listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "recent_edits"),
        "{listed}"
    );
    let call = |args: Value| {
        let r = s
            .handle(
                "tools/call",
                &json!({"name":"recent_edits","arguments":args}),
            )
            .unwrap();
        serde_json::from_str::<Value>(r["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let before = call(json!({}));
    let cursor = before["cursor"].as_u64().unwrap();
    // Written by "another tool": straight to disk, no Jazyk write path.
    std::fs::write(
        dir.join("docs/orders.md"),
        "# Orders\n\nAn order ships within two days.\n",
    )
    .unwrap();
    std::fs::write(dir.join("notes.txt"), "unselected\n").unwrap();
    let after = call(json!({"since":cursor}));
    let edits = after["edits"].as_array().unwrap();
    assert_eq!(edits.len(), 1, "{after}");
    assert_eq!(edits[0]["path"], "docs/orders.md");
    assert_eq!(edits[0]["removed"], false);
    assert_eq!(edits[0]["sets"], json!([{"setId":"ds_main","role":"docs"}]));
    assert!(
        edits[0]["goals"].as_array().is_some_and(|g| !g.is_empty()),
        "{after}"
    );
    assert_eq!(after["graphStale"], true);
    assert!(after["cursor"].as_u64().unwrap() > cursor);
    let again = call(json!({"since":after["cursor"]}));
    assert!(again["edits"].as_array().unwrap().is_empty(), "{again}");
    assert!(call(json!({"setId":"other"}))["edits"]
        .as_array()
        .unwrap()
        .is_empty());
    let ephemeral = McpServer::with_bridge(
        project.clone(),
        project.out.clone(),
        vec!["chat".into()],
        false,
        BridgeFlags {
            ephemeral: true,
            ..Default::default()
        },
    );
    let listed = ephemeral.handle("tools/list", &json!({})).unwrap();
    assert!(!listed["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"] == "recent_edits"));
}

#[test]
fn delegated_document_write_never_treats_unreadable_existing_bytes_as_missing() {
    let dir = bare_dir("delegated-unreadable-prose");
    crate::vfs::write(dir.join("docs/invalid.md"), [0xff, 0xfe]).unwrap();
    let project = crate::document_sets::Workspace::load(&dir)
        .unwrap()
        .files_project();
    let s = McpServer::with_bridge(
        project.clone(),
        project.out,
        vec!["chat".into()],
        false,
        BridgeFlags {
            ephemeral: true,
            edit_sink: Some("not-a-live-editor-sink".into()),
            ..Default::default()
        },
    );
    let error=refusal(s.handle("tools/call",&json!({"name":"write_document","arguments":{"path":"docs/invalid.md","text":"must not replace bytes"}})));
    assert!(error.contains("cannot read existing document"), "{error:?}");
    assert_eq!(
        crate::vfs::read(dir.join("docs/invalid.md")).unwrap(),
        vec![0xff, 0xfe]
    );
}

#[test]
fn chat_generation_configures_source_before_deriving_criteria() {
    let dir = bare_dir("chat-source-setup");
    let project = crate::project::Project::load(&dir);
    let mut store = Store::load(&project.out);
    store.graph.entities.insert(
        "ent:deck".into(),
        crate::model::Entity {
            name: "Deck".into(),
            ..Default::default()
        },
    );
    store.graph.requirements.insert(
        "req:deck".into(),
        crate::model::Requirement {
            statement: "The deck contains three slides.".into(),
            entities: vec!["ent:deck".into()],
            ..Default::default()
        },
    );
    store.save_for_test();
    let workspace = crate::document_sets::Workspace::load(&dir)
        .unwrap()
        .files_project();
    let server = McpServer::with_bridge(
        workspace.clone(),
        workspace.out,
        vec!["chat".into()],
        false,
        BridgeFlags::default(),
    );
    let call = |name, args| {
        server
            .handle("tools/call", &json!({"name":name,"arguments":args}))
            .unwrap()
    };
    let decode = |reply: Value| {
        serde_json::from_str::<Value>(reply["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    for name in ["binding_tasks", "generation_tasks"] {
        let reply = decode(call(name, json!({})));
        assert_eq!(reply["prerequisites"]["sourceMapping"], false, "{reply}");
        assert!(
            reply["next"]
                .as_str()
                .unwrap()
                .contains("update_document_set"),
            "{reply}"
        );
    }
    let board = crate::board::Board::compute(&project, &project.out);
    assert!(!board.goals.iter().any(|g| g.kind == "derive-criteria"));
    let bind = board.goals.iter().find(|g| g.kind == "bind").unwrap();
    assert_eq!(bind.change["sourceConfigured"], false);
    let before = decode(call("list_document_sets", json!({})));
    let updated = call(
        "update_document_set",
        json!({"baseHash":before["hash"],"set":{"id":"ds_main","source":["src/**","tests/**","build.py"]}}),
    );
    assert_ne!(updated["isError"], true, "{updated}");
    let after = decode(call("list_document_sets", json!({})));
    for key in ["id", "name", "docs", "roots"] {
        assert_eq!(before["sets"][0][key], after["sets"][0][key], "{key}");
    }
    assert_eq!(
        after["sets"][0]["source"],
        json!(["src/**", "tests/**", "build.py"])
    );
    let reply = decode(call("binding_tasks", json!({})));
    assert!(reply["prerequisites"]["sourceMapping"].is_null(), "{reply}");
    let project = crate::project::Project::load(&dir);
    let board = crate::board::Board::compute(&project, &project.out);
    assert!(board
        .goals
        .iter()
        .any(|g| g.kind == "derive-criteria" && g.target == "ent:deck"));
    assert!(
        !dir.join("src").vexists(),
        "Patterns may select future files"
    );
    crate::vfs::remove_dir_all(dir).ok();
}

#[test]
fn chat_release_unblocks_binding_and_empty_generation_names_prerequisites() {
    let dir = bare_dir("chat-release");
    crate::vfs::write(
        dir.join("docs/main.md"),
        "# Greeter\n\nThe greeter says hello.\n",
    )
    .unwrap();
    let project = crate::project::Project::load(&dir);
    let out = project.out.clone();
    let mut store = Store::load(&out);
    let (parsed, _) = crate::reconcile::parse_all(&project);
    store.sync_docs(&parsed);
    store.graph.entities.insert(
        "ent:greeter".into(),
        crate::model::Entity {
            name: "Greeter".into(),
            ..Default::default()
        },
    );
    store.graph.requirements.insert(
        "req:greet".into(),
        crate::model::Requirement {
            statement: "The greeter says hello.".into(),
            entities: vec!["ent:greeter".into()],
            ..Default::default()
        },
    );
    store.save_for_test();
    let server = chat_server(&dir);
    let call = |name, args| {
        server
            .handle("tools/call", &json!({"name":name,"arguments":args}))
            .unwrap()
    };
    let decode = |reply: Value| {
        serde_json::from_str::<Value>(reply["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let generation = decode(call("generation_tasks", json!({})));
    assert_eq!(generation["count"], 1, "{generation}");
    assert_eq!(generation["bindingWork"], 1, "{generation}");
    assert!(
        generation["prerequisites"]["binding"].is_null(),
        "{generation}"
    );
    assert!(generation["next"]
        .as_str()
        .unwrap()
        .contains("update_document_set"));
    assert_eq!(
        call("begin_binding", json!({"requirement":"req:greet"}))["isError"],
        true
    );
    let config = crate::vfs::read_to_string(dir.join(".jazyk/jazyk.toml")).unwrap();
    assert_eq!(
        call("release_work", json!({"stage":"publish"}))["isError"],
        true
    );
    crate::control::claim(&out, crate::control::BUILD_LEASE, "another-worker").unwrap();
    assert!(call("release_work", json!({"stage":"generate"}))
        .to_string()
        .contains("work-running"));
    crate::control::release_lease(&out, crate::control::BUILD_LEASE);
    assert_ne!(
        call("release_work", json!({"stage":"generate"}))["isError"],
        true
    );
    assert_eq!(
        crate::vfs::read_to_string(dir.join(".jazyk/jazyk.toml")).unwrap(),
        config
    );
    let binding = call("begin_binding", json!({"requirement":"req:greet"}));
    assert_ne!(binding["isError"], true, "{binding}");
    let reader = McpServer::with_bridge(
        project,
        out,
        vec!["graph".into()],
        false,
        BridgeFlags {
            reader: true,
            ..Default::default()
        },
    );
    assert!(!tool_names(&reader)
        .iter()
        .any(|n| n == "release_work" || n == "write_text_file"));
    let denied = reader.handle(
        "tools/call",
        &json!({"name":"release_work","arguments":{"stage":"generate"}}),
    );
    assert!(denied.is_err() || denied.unwrap()["isError"] == true);
    crate::vfs::remove_dir_all(dir).ok();
}

// The chat serving carries the two decree tools beside edit_fact: bump_limit
// records a node's own threshold with decree provenance as its own changeset
// and refuses a limit that does not apply; retract_decree deletes a node created
// by decree and refuses a quoted one. Mirrors docs/compiler/tools.md#chat-tools.
#[test]
fn chat_serving_bumps_limits_and_retracts_decrees() {
    use crate::model::{Entity, Provenance, SourceRef};
    use crate::store::{Commit, Op};
    let dir = bare_dir("chat-decree");
    crate::vfs::write(dir.join("docs/shop.md"), "# Shop\n\nAn Order is placed.\n").unwrap();
    let project = crate::project::Project::load(&dir);
    let out = project.out.clone();
    let (parsed, _) = crate::reconcile::parse_all(&project);
    let mut s = Store::load(&out);
    s.sync_docs(&parsed);
    s.apply(
        vec![
            Op::CreateEntity {
                id: "ent:order".into(),
                entity: Entity {
                    name: "Order".into(),
                    mentions: vec![SourceRef {
                        doc: "docs/shop.md".into(),
                        section: "/shop".into(),
                        quote: "An Order is placed.".into(),
                    }],
                    ..Default::default()
                },
            },
            Op::CreateEntity {
                id: "ent:fulfilment".into(),
                entity: Entity {
                    name: "Fulfilment".into(),
                    provenance: Some(Provenance::Decree {
                        author: "owner".into(),
                        at: "t".into(),
                        note: None,
                    }),
                    ..Default::default()
                },
            },
        ],
        &Commit::store("decree"),
    );
    let before = s.status.generation;
    drop(s);

    let server = chat_server(&dir);
    let names = tool_names(&server);
    for t in ["bump_limit", "retract_decree"] {
        assert!(
            names.iter().any(|n| n == t),
            "{} missing from {:?}",
            t,
            names
        );
    }

    // A bump lands as its own changeset with the raised threshold on the node.
    let v = server
        .bump_limit(&json!({"id": "ent:order", "limit": "requirements-per-entity", "value": 90}));
    assert_eq!(v["path"], "bump", "{}", v);
    let s = Store::load(&out);
    assert_eq!(s.status.generation, before + 1, "one changeset");
    assert_eq!(
        s.graph.entities["ent:order"].limits["requirements-per-entity"].value,
        90
    );
    drop(s);
    // A view limit on an entity is refused with the store's rule, nothing lands.
    let v = server.bump_limit(
        &json!({"id": "ent:order", "limit": "members-per-structural-view", "value": 40}),
    );
    assert_eq!(v["error"]["rule"], "bad-limit", "{}", v);
    assert_eq!(Store::load(&out).status.generation, before + 1);
    // A zero or missing value never reaches the store.
    let v = server
        .bump_limit(&json!({"id": "ent:order", "limit": "requirements-per-entity", "value": 0}));
    assert_eq!(v["error"]["rule"], "bad-value", "{}", v);

    // Retracting the decreed entity deletes it; the quoted one is not a decree.
    let v = server.retract_decree(&json!({"id": "ent:fulfilment", "reason": "never stated"}));
    assert_eq!(v["retracted"], true, "{}", v);
    let s = Store::load(&out);
    assert!(!s.graph.entities.contains_key("ent:fulfilment"));
    assert_eq!(s.status.generation, before + 2);
    drop(s);
    let v = server.retract_decree(&json!({"id": "ent:order", "reason": "no"}));
    assert_eq!(v["error"]["rule"], "not-a-decree", "{}", v);
    let s = Store::load(&out);
    assert!(s.graph.entities.contains_key("ent:order"));
    assert_eq!(s.status.generation, before + 2, "a refusal writes nothing");
    drop(s);
    let v = server.retract_decree(&json!({"id": "ent:nobody", "reason": "no"}));
    assert_eq!(v["error"]["rule"], "unknown-id", "{}", v);
    crate::vfs::remove_dir_all(&dir).ok();
}

// A chat dual write moves the prose and the graph in one changeset and absorbs
// its own hashes, so a following sync finds nothing dirty; edit_fact without an
// accepted sentence lands a decree with its proposal instead.
// Mirrors docs/frontends/acp.md#dual-write-tools.
#[test]
fn revise_requirement_commits_prose_and_graph_without_redirtying() {
    use crate::model::{Entity, Provenance, Requirement, SourceRef};
    use crate::store::{Commit, Op};
    let dir = std::env::temp_dir().join(format!("jazyk-mcp-revise-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&dir).ok();
    crate::vfs::create_dir_all(dir.join("docs")).unwrap();
    crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
    crate::vfs::write(
        dir.join("docs/pay.md"),
        "# Pay\n\nAn Order is paid within 30 days.\n",
    )
    .unwrap();
    let project = crate::project::Project::load(&dir);
    let out = project.out.clone();
    let (parsed, _) = crate::reconcile::parse_all(&project);
    let mut s = Store::load(&out);
    s.sync_docs(&parsed);
    let quote = "An Order is paid within 30 days.";
    s.apply(
        vec![
            Op::CreateEntity {
                id: "ent:order".into(),
                entity: Entity {
                    name: "Order".into(),
                    ..Default::default()
                },
            },
            Op::CreateRequirement {
                id: "req:pay-1".into(),
                requirement: Requirement {
                    statement: quote.into(),
                    entities: vec!["ent:order".into()],
                    source: Some(SourceRef {
                        doc: "docs/pay.md".into(),
                        section: "/pay".into(),
                        quote: quote.into(),
                    }),
                    ..Default::default()
                },
            },
        ],
        &Commit::store("session"),
    );
    let before = s.status.generation;
    drop(s);

    let server = chat_server(&dir);
    assert!(tool_names(&server).iter().any(|t| t == "edit_fact"));
    let new_text = "An Order is paid within 21 days.";
    let v = server.revise_requirement(
        &json!({"id": "req:pay-1", "new_text": new_text, "statement": new_text}),
    );
    assert_eq!(v["committed"], true, "{}", v);
    let text = crate::vfs::read_to_string(dir.join("docs/pay.md")).unwrap();
    assert!(text.contains(new_text), "{}", text);
    let mut s = Store::load(&out);
    assert_eq!(s.status.generation, before + 1, "one changeset");
    let r = &s.graph.requirements["req:pay-1"];
    assert_eq!(r.source.as_ref().unwrap().quote, new_text);
    assert_eq!(r.statement, new_text);
    let (parsed, _) = crate::reconcile::parse_all(&project);
    assert_eq!(s.docs["docs/pay.md"].content_hash, parsed["docs/pay.md"].0);
    let records = s.status.changes.clone();
    assert!(s.sync_docs(&parsed).is_empty(), "no re-dirtying");
    assert_eq!(s.status.changes, records);
    drop(s);

    // A decree through edit_fact lands graph-only with its proposal.
    let v = server.edit_fact(
            &json!({"id": "req:pay-1", "field": "facets", "value": [{"facet": "constraint", "reasoning": "an invariant"}]}),
        );
    assert_eq!(v["path"], "decree", "{}", v);
    let s = Store::load(&out);
    let r = &s.graph.requirements["req:pay-1"];
    assert!(r.source.is_none() && matches!(r.provenance, Some(Provenance::Decree { .. })));
    assert_eq!(r.facets.len(), 1);
    assert!(s
        .graph
        .diagnostics
        .values()
        .any(|d| d.rule == "ratification-pending"
            && d.lifecycle == "open"
            && d.subjects == vec!["req:pay-1".to_string()]
            && d.prompt.is_some()));
    assert!(s
        .status
        .has_change(crate::store::CHANGE_PROVENANCE_PENDING, "req:pay-1"));
    drop(s);
    // The prose form is refused now that the fact is decreed.
    let v = server.revise_requirement(&json!({"id": "req:pay-1", "new_text": "x"}));
    assert_eq!(v["error"]["rule"], "not-quoted");
    crate::vfs::remove_dir_all(&dir).ok();
}

// Chat can complete the batches it claims, without enabling direct graph surgery.
#[test]
fn chat_compiler_tools_stage_only_inside_the_claimed_batch() {
    let dir = bare_dir("chat-compiler-tools");
    crate::vfs::create_dir_all(dir.join("docs")).unwrap();
    crate::vfs::create_dir_all(dir.join(".jazyk")).unwrap();
    crate::vfs::write(
        dir.join(".jazyk/jazyk.toml"),
        "version=1\n[[sets]]\nid='ds_main'\nname='Main'\ndocs=['docs/**/*.md']\n",
    )
    .unwrap();
    crate::vfs::write(dir.join("docs/a.md"), "# A\n\nThe Greeter returns hello.\n").unwrap();
    let project = crate::project::Project::load(&dir);
    let out = project.out.clone();
    crate::vfs::create_dir_all(&out).unwrap();
    crate::control::Control {
        compile: "auto".into(),
        ..Default::default()
    }
    .save(&out);
    let server = McpServer::new(project, out.clone(), vec!["chat".into()], false);
    let names = tool_names(&server);
    for name in toolset("mcp-compile") {
        assert!(names.iter().any(|n| n == name), "missing {name}");
    }
    let call = |name: &str, args: Value| {
        server
            .handle_local("tools/call", &json!({"name":name,"arguments":args}))
            .unwrap()
    };
    let args = json!({"name":"Greeter", "mention":{
            "section":"docs/a.md#/a", "quote":"The Greeter returns hello."}});
    let rejected = call("upsert_entity", args.clone());
    assert_eq!(rejected["isError"], true);
    assert!(rejected["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("no-open-batch"));
    assert!(Store::load(&out).graph.entities.is_empty());
    let batch = server.begin_goals(&json!({"arguments":{}}));
    assert!(batch["error"].is_null(), "{batch}");
    let staged = call("upsert_entity", args);
    assert_ne!(staged["isError"], true, "{staged}");
    assert!(!server
        .open
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .session
        .staged
        .is_empty());
    assert!(
        Store::load(&out).graph.entities.is_empty(),
        "writes must remain staged"
    );
    let wrong = call(
        "report_diagnostic",
        json!({"rule":"contradiction", "message":"test"}),
    );
    assert_eq!(wrong["isError"], true);
    assert!(
        wrong["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("wrong-toolset"),
        "{wrong}"
    );
    server.abandon_goals(&json!({"arguments":{"reason":"test cleanup"}}));
    assert!(Store::load(&out).graph.entities.is_empty());
    assert!(crate::control::leases(&out).is_empty());
    crate::vfs::remove_dir_all(dir).unwrap();
}

// add_requirement's after_quote locates inside the named section: the same sentence
// in an earlier section is not the anchor, and a sentence found only elsewhere is
// refused. Mirrors docs/frontends/acp.md#dual-write-tools.
#[test]
fn add_requirement_anchors_after_quote_inside_its_section() {
    use crate::model::Entity;
    use crate::store::{Commit, Op};
    let dir = bare_dir("add-after-quote");
    let doc = "# Pay\n\n## Terms\n\nAn Order is paid on time.\n\nOnly terms say this.\n\n## Late\n\nAn Order is paid on time.\n\nLate fees apply.\n";
    crate::vfs::write(dir.join("docs/pay.md"), doc).unwrap();
    let project = crate::project::Project::load(&dir);
    let (parsed, _) = crate::reconcile::parse_all(&project);
    let mut s = Store::load(&project.out);
    s.sync_docs(&parsed);
    let late = s.docs["docs/pay.md"]
        .sections
        .keys()
        .find(|k| k.ends_with("late"))
        .cloned()
        .unwrap();
    // The entity needs a requirement holding it, or the commit's sweep drops it.
    let terms = s.docs["docs/pay.md"]
        .sections
        .keys()
        .find(|k| k.ends_with("terms"))
        .cloned()
        .unwrap();
    s.apply(
        vec![
            Op::CreateEntity {
                id: "ent:order".into(),
                entity: Entity {
                    name: "Order".into(),
                    ..Default::default()
                },
            },
            Op::CreateRequirement {
                id: "req:pay-1".into(),
                requirement: crate::model::Requirement {
                    statement: "An Order is paid on time.".into(),
                    entities: vec!["ent:order".into()],
                    source: Some(crate::model::SourceRef {
                        doc: "docs/pay.md".into(),
                        section: terms,
                        quote: "An Order is paid on time.".into(),
                    }),
                    ..Default::default()
                },
            },
        ],
        &Commit::store("session"),
    );
    drop(s);
    let server = chat_server(&dir);
    let elsewhere = server.add_requirement(&json!({"doc":"docs/pay.md","section":late,"text":"A late Order is flagged.",
            "statement":"A late Order is flagged.","entities":["ent:order"],"after_quote":"Only terms say this."}));
    assert_eq!(elsewhere["error"]["rule"], "stale-anchor", "{elsewhere}");
    assert_eq!(
        crate::vfs::read_to_string(dir.join("docs/pay.md")).unwrap(),
        doc
    );
    let v = server.add_requirement(&json!({"doc":"docs/pay.md","section":late,"text":"A late Order is flagged.",
            "statement":"A late Order is flagged.","entities":["ent:order"],"after_quote":"An Order is paid on time."}));
    assert_eq!(v["committed"], true, "{v}");
    let text = crate::vfs::read_to_string(dir.join("docs/pay.md")).unwrap();
    let (terms_at, late_at, added_at) = (
        text.find("## Terms").unwrap(),
        text.find("## Late").unwrap(),
        text.find("A late Order is flagged.").unwrap(),
    );
    assert!(terms_at < late_at && late_at < added_at, "{text}");
    assert!(
        text[late_at..]
            .starts_with("## Late\n\nAn Order is paid on time.\n\nA late Order is flagged."),
        "{text}"
    );
    crate::vfs::remove_dir_all(dir).ok();
}

// A delegated edit falls back to the disk only when the sink cannot take edits;
// a refusal or a missing acknowledgement fails the write and leaves the disk alone.
// Mirrors docs/frontends/acp.md#doc-edit-delegation.
#[cfg(unix)]
#[test]
fn delegated_edits_fall_back_only_when_the_editor_is_unavailable() {
    use std::io::{BufRead, BufReader, Write};
    let dir = bare_dir("sink-outcomes");
    let doc = dir.join("docs/a.md");
    crate::vfs::write(&doc, "# A\n").unwrap();
    let serve = |reply: &'static str| -> String {
        let path = format!(
            "/tmp/jz-sink-{}-{}.sock",
            crate::sys::process_id(),
            reply.len()
        );
        let _ = std::fs::remove_file(&path);
        let listener = crate::local_socket::UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let mut w = &stream;
            if !reply.is_empty() {
                writeln!(w, "{}", reply).unwrap();
            }
        });
        path
    };
    let with_sink = |sink: String| {
        McpServer::with_bridge(
            crate::project::Project::load(&dir),
            dir.join(".jazyk/ds_main"),
            vec!["chat".into()],
            false,
            BridgeFlags {
                edit_sink: Some(sink),
                ..Default::default()
            },
        )
    };
    let refused = with_sink(serve(r#"{"ok":false,"error":"the user declined"}"#)).write_edit(
        "docs/a.md",
        "# A\n",
        "# B\n",
        "# B\n",
    );
    assert!(refused.unwrap_err().contains("the user declined"));
    let unanswered = with_sink(serve("")).write_edit("docs/a.md", "# A\n", "# B\n", "# B\n");
    assert!(unanswered.is_err());
    assert_eq!(
        crate::vfs::read_to_string(&doc).unwrap(),
        "# A\n",
        "a failed delegation wrote the disk"
    );
    with_sink(serve(r#"{"ok":false,"unavailable":true,"error":"the client has no fs.writeTextFile capability"}"#))
            .write_edit("docs/a.md", "# A\n", "# B\n", "# B\n").unwrap();
    assert_eq!(crate::vfs::read_to_string(&doc).unwrap(), "# B\n");
    with_sink("/tmp/jz-sink-nobody-listens.sock".into())
        .write_edit("docs/a.md", "# B\n", "# C\n", "# C\n")
        .unwrap();
    assert_eq!(crate::vfs::read_to_string(&doc).unwrap(), "# C\n");
    crate::vfs::remove_dir_all(dir).ok();
}
