use super::*;

#[test]
fn project_context_is_workspace_wide_and_in_initialize() {
    let dir = bare_dir("integration-context");
    let project = crate::project::Project::load(&dir);
    let serving = McpServer::with_bridge(
        project.clone(),
        project.out,
        vec!["chat".into()],
        false,
        BridgeFlags {
            ephemeral: true,
            ..Default::default()
        },
    );
    let init = serving.handle("initialize", &json!({})).unwrap();
    assert!(init["instructions"]
        .as_str()
        .unwrap()
        .contains("Project integration context"));
    let catalog = serving.handle("tools/list", &json!({})).unwrap();
    assert_eq!(
        catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["name"] == "project_context")
            .count(),
        1
    );
    let reply = serving
        .handle(
            "tools/call",
            &json!({"name":"project_context","arguments":{}}),
        )
        .unwrap();
    let context: Value =
        serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(context["location"], "local");
    assert_eq!(context["github"], Value::Null);
    crate::vfs::remove_dir_all(dir).ok();
}

// A project tool is offered only where it can do something: scaffolding in a bare
// directory, settings in a project. The instructions say which case it is, so the
// agent never spends a call to learn it.
// Mirrors docs/frontends/acp.md#project-tools.
#[test]
fn project_tools_follow_the_project_state() {
    let base = std::env::temp_dir().join(format!("jazyk-mcp-init-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&base).ok();
    let bare = base.join("bare");
    let proj = base.join("proj");
    crate::vfs::create_dir_all(&bare).unwrap();
    crate::vfs::create_dir_all(&proj).unwrap();
    crate::vfs::write(
            { let path = proj.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();

    let bare_tools = tool_names(&chat_server(&bare));
    assert!(
        bare_tools.iter().any(|t| t == "init_project"),
        "{:?}",
        bare_tools
    );
    assert!(
        !bare_tools.iter().any(|t| t == "update_project_settings"),
        "{:?}",
        bare_tools
    );

    let proj_tools = tool_names(&chat_server(&proj));
    assert!(
        !proj_tools.iter().any(|t| t == "init_project"),
        "{:?}",
        proj_tools
    );
    assert!(
        proj_tools.iter().any(|t| t == "update_project_settings"),
        "{:?}",
        proj_tools
    );

    assert!(instructions_for(&["chat".to_string()], false, false).contains("NO PROJECT HERE"));
    // A tab's chat and reader say builds run from the Build button, since neither
    // can start one (docs/frontends/wasm.md#chat); a native chat has its own path.
    let note = "You cannot start a build: the Build button at the top of the page builds \
                    the documents into the graph. When asked to build, say so.";
    assert!(
        instructions_in(&["chat".to_string()], false, true, true).contains("The compile skill compiles")
    );
    assert!(reader_instructions().contains(note));
    assert!(!instructions_in(&["chat".to_string()], false, true, false).contains(note));
    assert!(instructions_for(&["chat".to_string()], false, true).contains("already initialized"));

    // The refusal survives as a floor under the listing: settings without a file.
    let r = chat_server(&bare).update_project_settings(&json!({"settings": {"llm.model": "x"}}));
    assert_eq!(r["error"]["rule"], "not-a-project");
    crate::vfs::remove_dir_all(&base).ok();
}

// Every reply's text is the JSON object pretty-printed, project tools included.
// Mirrors docs/frontends/mcp.md#toolsets.
#[test]
fn project_tool_replies_are_pretty_printed() {
    let dir = bare_dir("pretty-project-tools");
    let serving = workspace_chat(&dir, BridgeFlags::default());
    let reply = serving
        .handle(
            "tools/call",
            &json!({"name":"list_document_sets","arguments":{}}),
        )
        .unwrap();
    let text = reply["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("\n  \""), "{text}");
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["sets"][0]["id"], "ds_main");
    crate::vfs::remove_dir_all(dir).ok();
}

// project_context is project-wide: its schema takes no setId, while set tools do.
#[test]
fn project_context_schema_takes_no_set_id() {
    let dir = bare_dir("context-schema");
    let serving = workspace_chat(&dir, BridgeFlags::default());
    let list = serving.handle("tools/list", &json!({})).unwrap();
    let tool = |name: &str| {
        list["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .cloned()
            .unwrap()
    };
    let context = tool("project_context");
    assert!(
        context["inputSchema"]["properties"]["setId"].is_null(),
        "{context}"
    );
    assert_eq!(context["inputSchema"]["additionalProperties"], false);
    assert!(tool("search")["inputSchema"]["properties"]["setId"].is_object());
    crate::vfs::remove_dir_all(dir).ok();
}

// toml_set never panics on reachable input: a section path through a value, a key
// naming a table, or a file that does not parse comes back unchanged, and a missing
// section is appended as a table.
#[test]
fn toml_set_refuses_non_tables_without_panicking() {
    let base = "version = 1\nworkflow = \"manual\"\n";
    assert!(try_toml_set(base, "workflow", "compile", "auto")
        .unwrap_err()
        .contains("not a table"));
    assert_eq!(toml_set(base, "workflow", "compile", "auto"), base);
    assert_eq!(toml_set(base, "workflow.inner", "compile", "auto"), base);
    let nested = "version = 1\n[gen.worker]\nx = 1\n";
    assert!(try_toml_set(nested, "gen", "worker", "pipeline")
        .unwrap_err()
        .contains("is a table"));
    assert!(try_toml_set("version = [", "gen", "worker", "pipeline").is_err());
    let added = toml_set("version = 1\n", "workflow", "compile", "auto");
    assert!(added.contains("[workflow]\ncompile = \"auto\""), "{added}");
    let kept = toml_set(
        "version = 1 # keep\n\n[workflow]\ngenerate = \"manual\"\n",
        "workflow",
        "compile",
        "auto",
    );
    assert!(
        kept.contains("version = 1 # keep")
            && kept.contains("generate = \"manual\"")
            && kept.contains("compile = \"auto\""),
        "{kept}"
    );
}

// init_project reports a filesystem failure instead of panicking.
#[cfg(unix)]
#[test]
fn init_project_reports_an_unwritable_project_dir() {
    let base = std::env::temp_dir().join(format!(
        "jazyk-mcp-init-blocked-{}",
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&base).ok();
    crate::vfs::create_dir_all(&base).unwrap();
    // `.jazyk` exists as a file, so the config directory cannot be created.
    crate::vfs::write(base.join(".jazyk"), "not a directory").unwrap();
    let reply = chat_server(&base).init_project();
    assert_eq!(reply["error"]["rule"], "write-failed", "{reply}");
    crate::vfs::remove_dir_all(&base).ok();
}

// update_project_settings edits only the allowlisted keys, under the project lock,
// against the configuration hash, and never while a build runs.
// Mirrors docs/frontends/acp.md#project-tools.
#[test]
fn project_settings_need_a_hash_the_lock_and_no_build() {
    let dir = bare_dir("settings-guard");
    let serving = workspace_chat(&dir, BridgeFlags::default());
    let call = |args: Value| -> Value {
        let reply = serving
            .handle(
                "tools/call",
                &json!({"name":"update_project_settings","arguments":args}),
            )
            .unwrap();
        let body: Value =
            serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(reply["isError"], !body["error"].is_null(), "{reply}");
        body
    };
    let config = dir.join(crate::document_sets::CONFIG);
    let original = crate::vfs::read_to_string(&config).unwrap();
    let hash = crate::model::hash_hex(&original);
    for key in [
        "llm.base_url",
        "llm.model",
        "acp.agent",
        "executors.compile",
        "gen.deliverable",
        "gen.code",
    ] {
        assert_eq!(
            call(json!({"settings":{key:"x"},"baseHash":hash}))["error"]["rule"],
            "unsupported-key",
            "{key}"
        );
    }
    assert_eq!(
        call(json!({"settings":{"workflow.compile":"sometimes"},"baseHash":hash}))["error"]["rule"],
        "bad-value"
    );
    // Only a human switches a mode to auto (docs/compiler/control-plane.md#modes-and-releases).
    for key in ["workflow.compile", "workflow.generate"] {
        let refused = call(json!({"settings":{key:"auto"},"baseHash":hash}));
        assert_eq!(refused["error"]["rule"], "needs-human", "{key}");
        assert!(refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains(&format!("/config {key} auto")));
    }
    assert_eq!(
        call(json!({"settings":{"workflow.worker":"agent"}}))["error"]["rule"],
        "missing-argument"
    );
    assert_eq!(
        call(json!({"settings":{"workflow.worker":"agent"},"baseHash":"stale"}))["error"]["rule"],
        "stale-config"
    );
    {
        let _held = crate::document_sets::ProjectLock::acquire(&dir, "project.lock").unwrap();
        assert_eq!(
            call(json!({"settings":{"workflow.worker":"agent"},"baseHash":hash}))["error"]["rule"],
            "project-busy"
        );
    }
    let out = crate::document_sets::Workspace::load(&dir)
        .unwrap()
        .resolve(Some("ds_main"))
        .unwrap()
        .out;
    crate::control::claim(&out, crate::control::BUILD_LEASE, "internal-test").unwrap();
    assert_eq!(
        call(json!({"settings":{"workflow.worker":"agent"},"baseHash":hash}))["error"]["rule"],
        "build-running"
    );
    crate::control::release_lease(&out, crate::control::BUILD_LEASE);
    assert_eq!(
        crate::vfs::read_to_string(&config).unwrap(),
        original,
        "a refused edit wrote the file"
    );
    let ok = call(json!({"settings":{"workflow.worker":"agent"},"baseHash":hash}));
    let edited = crate::vfs::read_to_string(&config).unwrap();
    assert!(
        edited.contains("[workflow]\nworker = \"agent\""),
        "{edited}"
    );
    assert_eq!(ok["hash"], crate::model::hash_hex(&edited));
    crate::document_sets::Workspace::load(&dir).unwrap();
    crate::vfs::remove_dir_all(dir).ok();
}

// A jazyk.toml that does not load refuses tool calls but not the connection.
// Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
#[test]
fn a_broken_project_file_still_connects() {
    let dir = bare_dir("broken-config");
    let serving = workspace_chat(&dir, BridgeFlags::default());
    let scoped = chat_server(&dir);
    crate::vfs::write(
        dir.join(crate::document_sets::CONFIG),
        "version = 1\n[[sets]\n",
    )
    .unwrap();
    assert!(scoped.handle("initialize", &json!({})).is_ok());
    assert!(scoped.handle("tools/list", &json!({})).is_ok());
    let init = serving
        .handle("initialize", &json!({"protocolVersion":"2025-06-18"}))
        .unwrap();
    assert!(
        init["instructions"]
            .as_str()
            .unwrap()
            .contains("does not load"),
        "{init}"
    );
    assert_eq!(serving.handle("ping", &json!({})).unwrap(), json!({}));
    assert!(!serving.handle("tools/list", &json!({})).unwrap()["tools"]
        .as_array()
        .unwrap()
        .is_empty());
    let call = serving.handle(
        "tools/call",
        &json!({"name":"search","arguments":{"query":"x"}}),
    );
    assert!(call.is_err() || call.unwrap()["isError"] == true);
    crate::vfs::remove_dir_all(dir).ok();
}
