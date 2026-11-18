use super::*;

#[test]
fn unfinished_in_process_workers_preserve_staging_for_explicit_resume() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    for ending in ["end-turn", "model-error", "stop"] {
        let mut initial = Snapshot::new();
        initial.insert(".jazyk/jazyk.toml".into(), b"version=1\n[[sets]]\nid='ds_main'\nname='Main'\ndocs=['docs/**']\nroots=['docs/a.md']\nsource=[]\n".to_vec());
        initial.insert("docs/a.md".into(), b"# A\n\nNavigation index.\n".to_vec());
        initial.insert(
            ".jazyk/ds_main/local/control.yaml".into(),
            b"compile: auto\n".to_vec(),
        );
        let records = Arc::new(MemoryRecords::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let execution = Execution {
            model: "scripted-unfinished".into(),
            records: Some(records.clone()),
            cancel: cancelled,
            transport: Some(Arc::new(move |_| {
                if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"stage","type":"function","function":{"name":"set_coverage","arguments":json!({"section":"docs/a.md#/a","state":"non-normative","note":"Navigation index."}).to_string()}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":10}}).to_string());
                }
                if ending == "model-error" {
                    return Err("fixture model failure".into());
                }
                if ending == "stop" {
                    stop.store(true, Ordering::SeqCst);
                }
                Ok(json!({"choices":[{"message":{"role":"assistant","content":"Finished."},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":10}}).to_string())
            })),
            ..Default::default()
        };
        let mut workspace = Workspace::new(initial).unwrap();
        let result = workspace
            .request_test(
                "POST",
                "/api/jobs",
                r#"{"kind":"compile","sessions":1}"#,
                &execution,
            )
            .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            if ending == "end-turn" {
                2 + crate::limits::GRAPH_FINISH_REMINDERS as usize
            } else {
                2
            },
            "{ending}: {}",
            String::from_utf8_lossy(&result.body)
        );
        let record: crate::open_work::Record = {
            let data = records.0.lock().unwrap();
            let rows = data
                .iter()
                .filter(|((stream, _), _)| stream == "open-work")
                .collect::<Vec<_>>();
            assert_eq!(rows.len(), 1, "{ending}");
            serde_json::from_value(rows[0].1 .0.clone()).unwrap()
        };
        assert_eq!(record.state, "open", "{ending}");
        assert!(
            !record.payload["session"]["staged"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{ending}"
        );
        let files = workspace.snapshot();
        let _fs = crate::vfs::Scope::enter(&files);
        let _execution = crate::hosted::test_execution_scope(Execution {
            records: Some(records),
            ..Default::default()
        });
        let project = crate::project::Project::load(std::path::Path::new("/project"));
        let out = project.out.clone();
        let before = crate::store::Store::load(&out);
        assert!(
            !before.docs["docs/a.md"].coverage.contains_key("/a"),
            "{ending}"
        );
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
            assert_ne!(value["isError"], true, "{ending}: {value}");
            serde_json::from_str::<Value>(value["content"][0]["text"].as_str().unwrap()).unwrap()
        };
        let resumed = call(
            "begin_goals",
            json!({"workId":record.receipt.id,"ownerToken":record.receipt.owner}),
        );
        assert_eq!(resumed["resumed"], true, "{ending}: {resumed}");
        let claims = record.goals.iter().map(|goal| json!({"goal":goal,"justification":"The navigation index is marked non-normative."})).collect::<Vec<_>>();
        let done = call(
            "done",
            json!({"summary":"Commit explicitly resumed navigation work.","goals":claims}),
        );
        assert_eq!(done["committed"], true, "{ending}: {done}");
        assert!(crate::store::read_generation(&out) > before.status.generation);
    }
}

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

#[test]
fn graph_worker_continues_the_same_staged_batch_after_a_prose_reply() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let mut initial = Snapshot::new();
    initial.insert(".jazyk/jazyk.toml".into(), b"version=1\n[[sets]]\nid='ds_main'\nname='Main'\ndocs=['docs/**']\nroots=['docs/a.md']\nsource=[]\n".to_vec());
    initial.insert("docs/a.md".into(), b"# A\n\nNavigation index.\n".to_vec());
    initial.insert(
        ".jazyk/ds_main/local/control.yaml".into(),
        b"compile: auto\n".to_vec(),
    );
    let records = Arc::new(MemoryRecords::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let execution = Execution {
        model: "scripted-continue".into(),
        records: Some(records.clone()),
        transport: Some(Arc::new(move |body| {
            let n = seen.fetch_add(1, Ordering::SeqCst);
            if n == 1 {
                return Ok(model_reply("The navigation review is complete."));
            }
            let (name, args) = if n == 0 {
                (
                    "set_coverage",
                    json!({"section":"docs/a.md#/a","state":"non-normative","note":"Navigation index."}),
                )
            } else {
                assert_eq!(n, 2, "no extraction retry is needed");
                let request: Value = serde_json::from_str(body).unwrap();
                let messages = request["messages"].to_string();
                assert!(messages.contains("assigned graph batch is still open"));
                assert!(messages.contains("set_coverage"));
                assert!(messages.contains("stagedMutations"));
                (
                    "done",
                    json!({"summary":"Commit the completed navigation review.","goals":[{"goal":"g:reconcile-section:docs/a.md#/a","justification":"The navigation index is explicitly non-normative."}]}),
                )
            };
            Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":format!("step-{n}"),"type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":"tool_calls"}]}).to_string())
        })),
        ..Default::default()
    };
    let mut workspace = Workspace::new(initial).unwrap();
    let result = value(
        &mut workspace,
        "POST",
        "/api/jobs",
        json!({"kind":"compile","sessions":1}),
        &execution,
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3, "{result}");
    let rows = records.0.lock().unwrap();
    let work = rows
        .iter()
        .filter(|((stream, _), _)| stream == "open-work")
        .collect::<Vec<_>>();
    assert_eq!(
        work.len(),
        1,
        "the original work is committed, not restarted"
    );
    assert_eq!(work[0].1 .0["state"], "committed", "{result}");
    let files = workspace.snapshot();
    let _scope = crate::vfs::Scope::enter(&files);
    let project = crate::project::Project::load(std::path::Path::new("/project"));
    assert!(crate::store::Store::load(&project.out).docs["docs/a.md"]
        .coverage
        .contains_key("/a"));
}
