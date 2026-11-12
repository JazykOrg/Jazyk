use super::*;

#[test]
fn active_catalog_follows_batch_scope_and_keeps_hidden_write_gates() {
    let dir = bare_dir("active-catalog");
    crate::vfs::write(dir.join("docs/a.md"), "# A\n\nNavigation index.\n").unwrap();
    let project = crate::document_sets::Workspace::load(&dir)
        .unwrap()
        .resolve(Some("ds_main"))
        .unwrap();
    let out = project.out.clone();
    let mut control = crate::control::Control::load(&project, &out);
    control.compile = "auto".into();
    control.save(&out);
    let make = || {
        McpServer::with_bridge(
            project.clone(),
            out.clone(),
            vec!["compile".into()],
            true,
            BridgeFlags {
                ephemeral: true,
                ..Default::default()
            },
        )
    };
    let server = make();
    assert!(tool_names(&server)
        .iter()
        .any(|name| name == "upsert_entity"));
    server
        .handle("tools/call", &json!({"name":"begin_goals","arguments":{}}))
        .unwrap();
    // Every session kind from the captured build advertises its registry slice.
    for (kind, count) in [
        ("reconcile-section", 30),
        ("review-entity", 25),
        ("derive-criteria", 23),
        ("declare-edges", 20),
    ] {
        server
            .open
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .session
            .scope
            .goals[0]
            .kind = kind.into();
        let names = tool_names(&server);
        let mut expected = toolset_for_kinds(&[kind]);
        expected.extend(LIFECYCLE);
        expected.push("project_context");
        expected.extend(crate::agent_workflows::TOOLS);
        assert_eq!(
            names.len(),
            count + 1 + crate::agent_workflows::TOOLS.len(),
            "{kind}"
        );
        assert_eq!(
            names.into_iter().collect::<BTreeSet<_>>(),
            expected.into_iter().map(str::to_string).collect(),
            "{kind}"
        );
    }
    // Mixed batches expose the union, rather than the first goal's slice.
    {
        let mut open = server.open.lock().unwrap();
        let scope = &mut open.as_mut().unwrap().session.scope;
        let mut review = scope.goals[0].clone();
        review.kind = "review-entity".into();
        scope.goals.push(review);
    }
    assert!(tool_names(&server)
        .iter()
        .any(|name| name == "merge_entities"));
    server
        .open
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .session
        .scope
        .goals
        .pop();
    let names = tool_names(&server);
    let mut expected = toolset_for_kinds(&["declare-edges"]);
    expected.extend(LIFECYCLE);
    expected.push("project_context");
    expected.extend(crate::agent_workflows::TOOLS);
    assert_eq!(
        names.iter().cloned().collect::<BTreeSet<_>>(),
        expected.into_iter().map(str::to_string).collect()
    );
    assert!(!names.iter().any(|name| name == "upsert_entity"));
    let reply = server
        .handle(
            "tools/call",
            &json!({"name":"upsert_entity","arguments":{"name":"Hidden"}}),
        )
        .unwrap();
    assert_eq!(reply["isError"], true);
    assert!(reply["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("wrong-toolset"));
    // Recovery restores the scope before the next catalog is requested.
    let checkpoint = server.checkpoint().unwrap();
    drop(server);
    let recovered = make();
    recovered.restore(checkpoint).unwrap();
    assert_eq!(tool_names(&recovered), names);
    // The workspace parent advertises its active child's scope as well.
    let mut parent = crate::project::Project::load(&dir);
    parent.set_id = None;
    let workspace = McpServer::with_bridge(
        parent,
        dir.join(".jazyk"),
        vec!["chat".into()],
        true,
        BridgeFlags {
            ephemeral: true,
            ..Default::default()
        },
    );
    workspace
        .children
        .lock()
        .unwrap()
        .insert("ds_main".into(), recovered);
    assert_eq!(
        tool_names(&workspace).into_iter().collect::<BTreeSet<_>>(),
        names.into_iter().collect()
    );
    let abandoned = workspace.handle("tools/call", &json!({"name":"abandon_goals","arguments":{"setId":"ds_main","reason":"Test complete"}})).unwrap();
    assert_ne!(abandoned["isError"], true, "{abandoned}");
    assert!(tool_names(&workspace)
        .iter()
        .any(|name| name == "upsert_entity"));
    drop(workspace);
    crate::vfs::remove_dir_all(dir).unwrap();
}

// Every tool the AGENTS.md section names is one `jazyk mcp all` serves, so a
// rename breaks this test, not the agents that read the section.
// Mirrors docs/frontends/agent-context.md#the-jazyk-section-of-agentsmd.
#[test]
fn the_agents_md_section_names_only_served_tools() {
    let dir = bare_dir("agents-md-tools");
    // The project-level serving `jazyk mcp all` runs without --set.
    let project = crate::document_sets::Workspace::load(&dir)
        .unwrap()
        .files_project();
    let modes = ["compile", "generate", "verify", "decompile", "chat"]
        .map(String::from)
        .to_vec();
    let server = McpServer::with_bridge(
        project,
        dir.join(".jazyk"),
        modes,
        false,
        BridgeFlags::default(),
    );
    let names = tool_names(&server);
    let payload = include_str!(concat!(
        env!("OUT_DIR"),
        "/payloads/goals/prompts/conversation/agent-main.md.mustache"
    ));
    let named: Vec<&str> = payload
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|t| t.contains('_') && t.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
        .collect();
    assert!(named.len() >= 4, "{named:?}");
    for name in named {
        assert!(
            names.iter().any(|n| n == name),
            "`{name}` is not served: {names:?}"
        );
    }
    let init = server.handle("initialize", &json!({})).unwrap();
    assert!(init["instructions"]
        .as_str()
        .unwrap()
        .contains("Active tool packages and enforced"));
    crate::vfs::remove_dir_all(dir).ok();
}

#[test]
fn runtime_serving_needs_no_graph_and_exposes_execution_and_context() {
    let dir = bare_dir("runtime-only");
    let project = crate::project::Project::load(&dir);
    let server = McpServer::with_bridge(
        project,
        dir.join("state"),
        vec!["runtime".into()],
        false,
        BridgeFlags::default(),
    );
    let names = tool_names(&server);
    assert_eq!(names.len(), crate::task_runtime::TOOLS.len() + 1);
    assert!(names.iter().any(|n| n == "project_context"));
    for name in crate::task_runtime::TOOLS {
        assert!(names.iter().any(|n| n == name));
    }
    let init = server.handle("initialize", &json!({})).unwrap();
    assert!(init["instructions"]
        .as_str()
        .unwrap()
        .starts_with(crate::task_runtime::PROMPT));
    assert!(init["instructions"]
        .as_str()
        .unwrap()
        .contains("Project integration context"));
    let tools = server.handle("tools/list", &json!({})).unwrap();
    assert!(tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["inputSchema"]["properties"]["setId"].is_null()));
    crate::vfs::remove_dir_all(dir).ok();
}

// A reader conversation (a small model in a tab) gets the read tools alone and
// the short instruction block; a write asked of it is refused.
// Mirrors docs/frontends/wasm.md#chat.
#[test]
fn reader_serving_lists_only_the_read_tools() {
    let dir = bare_dir("reader");
    let project = crate::project::Project::load(&dir);
    let out = project.out.clone();
    let s = McpServer::with_bridge(
        project,
        out,
        vec!["graph".to_string()],
        false,
        BridgeFlags {
            ephemeral: true,
            reader: true,
            ..Default::default()
        },
    );
    let init = s
        .handle("initialize", &json!({"protocolVersion": "2024-11-05"}))
        .unwrap();
    let instructions = init["instructions"].as_str().unwrap();
    assert!(
        instructions.contains("can only read the project"),
        "{}",
        instructions
    );
    assert!(
        !instructions.contains("The write tools are"),
        "{}",
        instructions
    );
    assert_eq!(
        tool_names(&s),
        crate::tools::READ_TOOLS
            .iter()
            .copied()
            .chain(std::iter::once("project_context"))
            .chain(crate::agent_workflows::TOOLS)
            .map(str::to_string)
            .collect::<Vec<_>>()
    );
    let r = s.handle(
        "tools/call",
        &json!({"name": "upsert_entity", "arguments": {"name": "X", "stereotype": "system"}}),
    );
    assert!(r.is_err() || r.unwrap()["isError"].as_bool().unwrap_or(false));
}

#[test]
fn project_reader_keeps_read_only_instructions_and_set_discovery() {
    let dir = bare_dir("project-reader");
    let project = crate::document_sets::Workspace::load(&dir)
        .unwrap()
        .files_project();
    let s = McpServer::with_bridge(
        project.clone(),
        project.out,
        vec!["graph".into()],
        false,
        BridgeFlags {
            ephemeral: true,
            reader: true,
            ..Default::default()
        },
    );
    let init = s.handle("initialize", &json!({})).unwrap();
    let instructions = init["instructions"].as_str().unwrap();
    assert!(instructions.contains("can only read the project"));
    assert!(instructions.contains("list_document_sets"));
    assert!(!instructions.contains("The write tools are"));
    let names = tool_names(&s);
    assert!(names.iter().any(|name| name == "list_document_sets"));
    assert!(names.iter().any(|name| name == "read_document"));
    assert!(names.iter().all(
        |name| crate::agent_workflows::TOOLS.contains(&name.as_str())
            || name == "list_document_sets"
            || name == "read_document"
            || name == "project_context"
            || crate::tools::READ_TOOLS.contains(&name.as_str())
    ));
    #[cfg(all(feature = "native-documents", not(target_arch = "wasm32")))]
    {
        crate::vfs::write(
            dir.join("docs/contract.docx"),
            include_bytes!("../../../tests/fixtures/document-proof/contract.docx"),
        )
        .unwrap();
        let result = s
            .handle(
                "tools/call",
                &json!({"name":"read_document", "arguments":{"path":"docs/contract.docx"}}),
            )
            .unwrap();
        let value: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(value["projection"]["text"]
            .as_str()
            .unwrap()
            .contains("The gateway shall return"));
        assert!(value.get("base64").is_none());
        assert_eq!(value["diagnostics"][0]["code"], "broken-link");
        assert!(!refusal(s.handle(
            "tools/call",
            &json!({"name":"read_document", "arguments":{"path":"../outside.docx"}})
        ))
        .is_empty());
    }
}

#[test]
fn conversational_contract_does_not_require_discovery_or_execution() {
    let dir = bare_dir("shared-conversation-policy");
    let instructions = chat_server(&dir).handle("initialize", &json!({})).unwrap();
    let instructions = instructions["instructions"].as_str().unwrap();
    assert!(instructions.contains("self-contained general question directly"));
    assert!(instructions.contains("A docs edit does not authorize"));
    assert!(instructions.contains("read_jazyk_skill"));
    assert!(!instructions.contains("GENERATION LOOP:"));
    assert!(!instructions.contains("Read task_context and choose"));
    assert!(!instructions.contains("The write tools are:"));
}

#[test]
fn workflow_policy_and_skills_match_agents_mcp_acp_and_restricted_servings() {
    let dir = bare_dir("workflow-parity");
    let config = dir.join(crate::document_sets::CONFIG);
    let mut text = crate::vfs::read_to_string(&config).unwrap();
    text.push_str(
        "\n[[sets]]\nid = 'ds_other'\nname = 'Other'\ndocs = ['other/**']\nsource = []\n",
    );
    crate::vfs::write(&config, text).unwrap();
    crate::agents_md::sync(&dir);
    let policy = crate::agent_workflows::render(&dir).unwrap();
    let agents = crate::vfs::read_to_string(dir.join("AGENTS.md")).unwrap();
    assert!(agents.contains(&format!("\n{policy}<!-- JAZYK:END -->")));
    for (modes, reader, scoped) in [
        (vec!["chat"], false, false),
        (
            vec!["compile", "generate", "verify", "decompile", "chat"],
            false,
            false,
        ),
        (vec!["graph"], true, false),
        (vec!["compile"], false, true),
    ] {
        let workspace = crate::document_sets::Workspace::load(&dir).unwrap();
        let project = if scoped {
            workspace.resolve(Some("ds_main")).unwrap()
        } else {
            workspace.files_project()
        };
        // Ephemeral compile/chat are also the ACP-injected serving profiles.
        let server = McpServer::with_bridge(
            project.clone(),
            project.out.clone(),
            modes.into_iter().map(String::from).collect(),
            false,
            BridgeFlags {
                ephemeral: true,
                reader,
                ..Default::default()
            },
        );
        let init = server.handle("initialize", &json!({})).unwrap();
        let instructions = init["instructions"].as_str().unwrap();
        assert!(instructions.starts_with(&policy));
        assert!(instructions.contains("Jazyk project filesystem root (JSON string):"));
        assert!(instructions.contains(&json!(dir.to_string_lossy()).to_string()));
        assert!(instructions.contains("`content/docs/index.md`"));
        assert!(instructions.contains("`../docs/index.md` resolves to `docs/index.md`"));
        assert!(instructions.contains("It does not prove the link's intended target is missing"));
        let catalog = server.handle("tools/list", &json!({})).unwrap();
        for name in crate::agent_workflows::TOOLS {
            let tool = catalog["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == name)
                .unwrap();
            assert!(tool["inputSchema"]["properties"]["setId"].is_null());
        }
        let listed = server
            .handle(
                "tools/call",
                &json!({"name":"list_jazyk_skills","arguments":{}}),
            )
            .unwrap();
        let listed: Value =
            serde_json::from_str(listed["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(listed, crate::agent_workflows::catalog().unwrap());
        for skill in crate::agent_workflows::SKILLS {
            let reply = server
                .handle(
                    "tools/call",
                    &json!({"name":"read_jazyk_skill","arguments":{"name":skill.name}}),
                )
                .unwrap();
            let reply: Value =
                serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap();
            assert_eq!(reply["body"], skill.prepared);
        }
        assert!(!refusal(server.handle(
            "tools/call",
            &json!({"name":"read_jazyk_skill","arguments":{"name":"../../secret"}})
        ))
        .is_empty());
    }
    crate::vfs::remove_file(config).unwrap();
    let uninitialized = chat_server(&dir);
    assert!(
        uninitialized.handle("initialize", &json!({})).unwrap()["instructions"]
            .as_str()
            .unwrap()
            .contains("NO PROJECT HERE")
    );
    assert!(tool_names(&uninitialized).contains(&"read_jazyk_skill".to_string()));
    assert!(uninitialized
        .handle(
            "tools/call",
            &json!({"name":"read_jazyk_skill","arguments":{"name":"change-code"}})
        )
        .is_ok());
    crate::vfs::remove_dir_all(dir).unwrap();
}

// A verify serving for an agent with no editor of its own lists the read-only
// file tools and never the write. Mirrors docs/compiler/goals/verify.md#tools.
#[test]
fn verify_serving_lists_read_only_file_tools_when_serving_files() {
    let dir = bare_dir("verify-files");
    let project = crate::project::Project::load(&dir);
    let out = project.out.clone();
    let serve = |serve_files: bool| {
        McpServer::with_bridge(
            project.clone(),
            out.clone(),
            vec!["verify".to_string()],
            false,
            BridgeFlags {
                serve_files,
                ..Default::default()
            },
        )
    };
    let with = tool_names(&serve(true));
    for t in [
        "read_text_file",
        "list_files",
        "run_task",
        "run_tests",
        "cancel_run",
        "read_run_log",
    ] {
        assert!(with.iter().any(|n| n == t), "{} missing from {:?}", t, with);
    }
    assert!(
        !with
            .iter()
            .any(|n| matches!(n.as_str(), "write_text_file" | "run_command")),
        "{:?}",
        with
    );
    let without = tool_names(&serve(false));
    assert!(
        !without.iter().any(|n| n == "read_text_file"),
        "{:?}",
        without
    );
    crate::vfs::remove_dir_all(&dir).ok();
}

// Every serving lists each tool name once: the catalog's `done` and the
// lifecycle's `done` are one entry, and a union of modes never repeats a shared
// tool. Claude Code forwards the list as is and its API refuses a repeated name.
#[test]
fn tool_listings_never_repeat_a_name() {
    let dir = bare_dir("unique-names");
    let project = crate::project::Project::load(&dir);
    let out = project.out.clone();
    for modes in [
        vec!["compile"],
        vec!["chat"],
        vec!["compile", "chat"],
        vec!["generate", "verify"],
        vec!["benchmark"],
        vec!["graph"],
    ] {
        let s = McpServer::with_bridge(
            project.clone(),
            out.clone(),
            modes.iter().map(|m| m.to_string()).collect(),
            false,
            BridgeFlags {
                serve_files: true,
                only: Some("b1-1".into()),
                ..Default::default()
            },
        );
        let names = tool_names(&s);
        let unique: std::collections::BTreeSet<&String> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "{:?}: {:?}", modes, names);
        if modes.contains(&"compile") {
            assert!(names.iter().any(|n| n == "done"), "{:?}", names);
        }
    }
    crate::vfs::remove_dir_all(&dir).ok();
}

// The chat serving carries the compilation lifecycle beside the ledger
// lifecycles. Mirrors docs/compiler/tools.md#toolsets.
#[test]
fn chat_serving_lists_the_compilation_lifecycle() {
    let dir = bare_dir("chat-lifecycle");
    let names = tool_names(&chat_server(&dir));
    for t in [
        "goals",
        "begin_goals",
        "done",
        "abandon_goals",
        "begin_binding",
        "begin_generation",
        "begin_verification",
        "release_work",
        "read_text_file",
        "write_text_file",
        "list_files",
    ] {
        assert!(
            names.iter().any(|n| n == t),
            "{} missing from {:?}",
            t,
            names
        );
    }
    assert!(names.iter().any(|n| n == "upsert_entity"), "{:?}", names);
    assert!(!names.iter().any(|n| n == "run_command"), "{:?}", names);
    crate::vfs::remove_dir_all(&dir).ok();
}

// The compile serving lists the goal lifecycle, the view tools, and the goal
// tools; a raw write tool outside an open batch is rejected toward begin_goals.
// Mirrors docs/compiler/tools.md#toolsets.
#[test]
fn compile_serving_lists_goal_and_view_tools_and_gates_writes() {
    let dir = std::env::temp_dir().join(format!("jazyk-mcp-toolset-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&dir).ok();
    crate::vfs::create_dir_all(dir.join("docs")).unwrap();
    crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
    let project = crate::project::Project::load(&dir);
    let out = project.out.clone();
    let server = McpServer::new(project, out, vec!["compile".into()], false);
    let names = tool_names(&server);
    for t in [
        "goals",
        "begin_goals",
        "done",
        "abandon_goals",
        "upsert_view",
        "update_view",
        "delete_view",
        "mark_goal_done",
        "mark_goal_failed",
        "settle_pairs",
        "load_skill",
        "load",
        "unload",
        "graph_status",
    ] {
        assert!(names.iter().any(|n| n == t), "{} missing: {:?}", t, names);
    }
    for legacy in [
        "compilation_tasks",
        "begin_compilation",
        "finish_compilation",
        "abandon_compilation",
    ] {
        assert!(
            !names.iter().any(|n| n == legacy),
            "{} should be gone",
            legacy
        );
    }
    // A raw write outside an open batch is rejected toward begin_goals.
    let r = server
        .handle(
            "tools/call",
            &json!({"name": "upsert_entity", "arguments": {"name": "Thing",
                    "mention": {"section": "docs/a.md#/a", "quote": "x"}}}),
        )
        .unwrap();
    assert_eq!(r["isError"], true);
    let text = r["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("no-open-batch") && text.contains("begin_goals"),
        "{}",
        text
    );
    crate::vfs::remove_dir_all(&dir).ok();
}

// A reader serving lists and serves reads alone even in chat mode: no dual write,
// project tool, decree tool, or lifecycle reaches it. Mirrors docs/frontends/wasm.md#chat.
#[test]
fn a_reader_chat_serving_offers_and_serves_no_writes() {
    let dir = bare_dir("reader-chat");
    crate::vfs::write(dir.join("docs/a.md"), "# A\n\nThe tool sorts lines.\n").unwrap();
    let reader = workspace_chat(
        &dir,
        BridgeFlags {
            ephemeral: true,
            reader: true,
            ..Default::default()
        },
    );
    let names = tool_names(&reader);
    let writes = [
        "revise_requirement",
        "add_requirement",
        "retract_requirement",
        "edit_fact",
        "retract_decree",
        "bump_limit",
        "answer_diagnostic",
        "update_diagnostic",
        "update_project_settings",
        "init_project",
        "begin_goals",
        "done",
        "goals",
        "abandon_goals",
        "release_work",
        "write_document",
    ];
    for name in writes {
        assert!(!names.iter().any(|n| n == name), "{name} listed: {names:?}");
    }
    let config = crate::vfs::read_to_string(dir.join(crate::document_sets::CONFIG)).unwrap();
    for (name, args) in [
        (
            "update_project_settings",
            json!({"settings":{"workflow.compile":"auto"},"baseHash":crate::model::hash_hex(&config),"setId":"ds_main"}),
        ),
        (
            "add_requirement",
            json!({"doc":"docs/a.md","section":"/a","text":"It must not land.","statement":"x","entities":[],"setId":"ds_main"}),
        ),
        ("begin_goals", json!({"setId":"ds_main"})),
    ] {
        let refused = reader.handle("tools/call", &json!({"name":name,"arguments":args}));
        assert!(
            refused.is_err() || refused.as_ref().unwrap()["isError"] == true,
            "{name}: {refused:?}"
        );
    }
    assert_eq!(
        crate::vfs::read_to_string(dir.join(crate::document_sets::CONFIG)).unwrap(),
        config
    );
    assert_eq!(
        crate::vfs::read_to_string(dir.join("docs/a.md")).unwrap(),
        "# A\n\nThe tool sorts lines.\n"
    );
    crate::vfs::remove_dir_all(dir).ok();
}
