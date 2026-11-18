use super::*;

#[test]
fn staged_work_replays_when_host_dies_before_workspace_checkpoint_and_compacts_after_ack() {
    let mut initial = Snapshot::new();
    initial.insert(".jazyk/jazyk.toml".into(),b"version = 1\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = [\"docs/a.md\"]\nsource = []\n".to_vec());
    initial.insert("docs/a.md".into(), b"# A\n\nNavigation index.\n".to_vec());
    let records = Arc::new(MemoryRecords::default());
    let execution = Execution {
        records: Some(records.clone()),
        ..Default::default()
    };
    let mut work_id = String::new();
    let mut owner = Value::Null;
    for attempt in 0..2 {
        let fs = crate::vfs::Scope::enter(&initial);
        let _execution = crate::hosted::execution::test_execution_scope(execution.clone());
        let project = crate::document_sets::Workspace::load(std::path::Path::new("/project"))
            .unwrap()
            .resolve(Some("ds_main"))
            .unwrap();
        let out = project.out.clone();
        let mut control = crate::control::Control::load(&project, &out);
        control.compile = "auto".into();
        control.save(&out);
        let server = crate::mcp::McpServer::with_bridge(
            project,
            out.clone(),
            vec!["compile".into()],
            true,
            crate::mcp::BridgeFlags {
                ephemeral: true,
                ..Default::default()
            },
        );
        let call = |name: &str, args: Value| {
            let value = server
                .handle("tools/call", &json!({"name":name,"arguments":args}))
                .unwrap();
            assert_ne!(value["isError"], true, "{value}");
            serde_json::from_str::<Value>(value["content"][0]["text"].as_str().unwrap()).unwrap()
        };
        let opened = if attempt == 0 {
            call("begin_goals", json!({}))
        } else {
            call("begin_goals", json!({"workId":work_id,"ownerToken":owner}))
        };
        if attempt == 0 {
            work_id = opened["workId"].as_str().unwrap().into();
            owner = opened["ownerToken"].clone();
            call(
                "set_coverage",
                json!({"section":"docs/a.md#/a","state":"non-normative","note":"Durable navigation."}),
            );
            let goals = crate::open_work::read(&out, &work_id).unwrap().goals;
            let claims = goals
                .iter()
                .map(|g| json!({"goal":g,"justification":"Navigation index marked non-normative."}))
                .collect::<Vec<_>>();
            let done = call(
                "done",
                json!({"summary":"Commit navigation.","goals":claims}),
            );
            assert_eq!(done["committed"], true, "{done}");
            // No host save acknowledges this candidate. The next attempt gets only
            // the original workspace and the durable record store.
            confirm_open_work(&initial, records.as_ref()).unwrap();
            let data = records.0.lock().unwrap();
            assert!(data
                .values()
                .any(|(meta, _)| meta["state"] == "committed" && !meta["payload"].is_null()));
        } else {
            assert_eq!(opened["resumed"], true, "{opened}");
            let done = call("done", json!({"summary":"Replay retained commit."}));
            assert_eq!(done["committed"], true, "{done}");
            confirm_open_work(&fs.snapshot(), records.as_ref()).unwrap();
            let data = records.0.lock().unwrap();
            assert!(data.values().all(|(meta, _)| meta["payload"].is_null()));
        }
    }
}
