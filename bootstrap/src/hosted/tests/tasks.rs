// Task runs through the host runtime transport.
use super::*;

#[test]
fn shared_task_suite_dispatches_once_per_pass_and_keeps_case_and_owner_evidence_separate() {
    use crate::harness::Recovery;
    use crate::{
        gen::{mark as gen_mark, GenSettings, Ledger},
        model::{hash_hex, Criterion, Entity, Requirement},
        store::Store,
        verify::{run_selected, status_of},
    };
    use base64::Engine;
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };

    let submissions = Arc::new(Mutex::new(BTreeMap::<String, Value>::new()));
    for owner in ["owner-a", "owner-b"] {
        let _fs = crate::vfs::Scope::enter(&Default::default());
        let _state = crate::scoped_state::Scope::enter();
        let root = std::path::Path::new("/project");
        let out = root.join(".jazyk/ds_main");
        crate::vfs::create_dir_all(root.join(".jazyk")).unwrap();
        let mut store = Store {
            out: out.clone(),
            ..Default::default()
        };
        store.graph.entities.insert(
            "ent:cart".into(),
            Entity {
                name: "Cart".into(),
                ..Default::default()
            },
        );
        store.graph.requirements.insert(
            "req:shop-1".into(),
            Requirement {
                statement: "The Cart shall hold items.".into(),
                entities: vec!["ent:cart".into()],
                ..Default::default()
            },
        );
        crate::vfs::write(root.join(".jazyk/jazyk.toml"), "version=1\n[[sets]]\nid='ds_main'\nname='Site'\ndocs=['docs/**']\nsource=['src/**','tests/**']\n[[tasks]]\nid='check'\nname='Shared suite'\ndescription='Check related cases together'\ncommand='pytest --junitxml=reports/check.xml'\nreports=['reports/*.xml']\n[[tasks]]\nid='other'\nname='Other suite'\ndescription='Unselected checks'\ncommand='false'\n").unwrap();
        let project = crate::document_sets::Workspace::load(root)
            .unwrap()
            .resolve(Some("ds_main"))
            .unwrap();
        let gs = GenSettings::resolve(&project);
        crate::vfs::create_dir_all(root.join("src")).unwrap();
        crate::vfs::create_dir_all(root.join("tests")).unwrap();
        crate::vfs::write(root.join("tests/suite.py"), "def works(): pass\ndef broken(): pass\ndef skipped(): pass\ndef absent(): pass\ndef unrelated(): pass\n").unwrap();
        let requirement = store.graph.requirements["req:shop-1"].clone();
        for id in ["req:shop-2", "req:unrelated"] {
            store.graph.requirements.insert(
                id.into(),
                Requirement {
                    statement: format!("Statement for {id}"),
                    ..requirement.clone()
                },
            );
        }
        crate::vfs::write(
            root.join("src/cart.rs"),
            format!("// {owner}: revision 1\n"),
        )
        .unwrap();
        gen_mark(&store, "ent:cart", None, &json!({"files":["src/cart.rs"],"tests":[
            {"requirement":"req:shop-1","kind":"programmatic","artifact":"tests/suite.py","name":"Site::works","run":"task:check"},
            {"requirement":"req:shop-2","kind":"programmatic","artifact":"tests/suite.py","name":"Other::works","run":"task:check"},
            {"requirement":"req:unrelated","kind":"programmatic","artifact":"tests/suite.py","name":"unrelated","run":"task:other"}
        ]}), &gs).unwrap();
        let mut ledger = Ledger::load(&out);
        for (rid, cid, selector) in [
            ("req:shop-1", "crit:pass", "Site::works"),
            ("req:shop-1", "crit:fail", "Site::broken"),
            ("req:shop-1", "crit:skip", "Site::skipped"),
            ("req:shop-2", "crit:other", "Other::works"),
            ("req:shop-2", "crit:missing", "Site::absent"),
            ("req:shop-2", "crit:ambiguous", "works"),
        ] {
            store.graph.criteria.insert(
                cid.into(),
                Criterion {
                    requirement: rid.into(),
                    statement: cid.into(),
                    ..Default::default()
                },
            );
            let row = ledger.requirements.get_mut(rid).unwrap();
            let mut test = row.test.clone();
            test.name = selector.into();
            row.tests.insert(
                cid.into(),
                crate::gen::CritTest {
                    test,
                    criterion_hash: hash_hex(cid),
                    test_hash: row.hashes.test.clone(),
                    verdict: "pass".into(),
                    last_run: None,
                    exit_code: None,
                    evidence: None,
                },
            );
        }
        ledger.entities.get_mut("cart").unwrap().fact_hash =
            crate::gen::fact_hash(&store, "ent:cart");
        let unrelated = serde_json::to_value(&ledger.requirements["req:unrelated"]).unwrap();
        ledger.save(&out).unwrap();
        let requests = submissions.clone();
        let runtime: crate::hosted::RuntimeTransport = Arc::new(move |request| {
            let run_id = request["run_id"].as_str().unwrap_or_default();
            match request["operation"].as_str().unwrap() {
                "task_context" => Ok(json!({"available":true})),
                "run_task" => {
                    assert_eq!(request["task"]["id"], "check");
                    let mut requests = requests.lock().unwrap();
                    let id = format!("{owner}-run-{}", requests.len() + 1);
                    requests.insert(id.clone(), request.clone());
                    Ok(json!({"id":id}))
                }
                "wait_run" => {
                    assert!(run_id.starts_with(owner));
                    assert!(requests.lock().unwrap().contains_key(run_id));
                    Ok(json!({"terminated":true}))
                }
                "result" => {
                    assert!(run_id.starts_with(owner));
                    let submitted = requests.lock().unwrap()[run_id].clone();
                    let task: crate::task_runtime::Task =
                        serde_json::from_value(submitted["task"].clone()).unwrap();
                    let input = serde_json::from_value(submitted["files"].clone()).unwrap();
                    let xml = "<testsuite><testcase classname='Site' name='works'/><testcase classname='Site' name='broken'><failure/></testcase><testcase classname='Site' name='skipped'><skipped/></testcase><testcase classname='Other' name='works'/><testcase classname='Unbound' name='noise'/></testsuite>";
                    let raw = json!({"status":"failed","complete":true,"exit_code":1,"log":"fixture suite completed","files":{},"cache":{},"deleted":[],"products":[],"reports":{"reports/check.xml":base64::engine::general_purpose::STANDARD.encode(xml)}});
                    let mut result =
                        crate::task_runtime::validate_result(&task, &input, raw).unwrap();
                    result["input_files"] = submitted["files"].clone();
                    Ok(result)
                }
                operation => panic!("Unexpected suite operation {operation}"),
            }
        });
        let saved = Arc::new(Mutex::new(None::<(Value, Snapshot)>));
        let checkpoint = saved.clone();
        let sink: CheckpointSink = Arc::new(move |mut capsule, files| {
            let checkpoint = checkpoint.clone();
            Box::pin(async move {
                let next = capsule["sequence"].as_u64().unwrap() + 1;
                capsule["sequence"] = json!(next);
                *checkpoint.lock().unwrap() = Some((capsule, files));
                Ok(next)
            })
        });
        let _execution = ExecutionScope::enter(Execution {
            runtime: Some(runtime),
            checkpoint: Some(sink),
            ..Default::default()
        });
        let _recovery = crate::session_recovery::Scope::enter(None);
        let parent_key = crate::session_recovery::command_key("compiler-progress");
        for revision in [1, 2] {
            crate::vfs::write(
                root.join("src/cart.rs"),
                format!("// {owner}: revision {revision}\n"),
            )
            .unwrap();
            let before = submissions.lock().unwrap().len();
            // Deliberately reuse these ids in the other owner's scope.
            let invocation = format!("run-tests-{revision}");
            let state: crate::harness::HarnessState = serde_json::from_value(json!({
                "version":1,"run_id":"suite-worker","transition":revision,"rounds":revision,
                "conversation":[],"model_receipts":{},"tool_receipts":{},
                "pending_tool":{"id":invocation,"name":"run_tests","args":{}},
                "delivered_inputs":[],"usage":[]
            }))
            .unwrap();
            let mut recovery =
                crate::session_recovery::McpRecovery::utility("suite-worker".into()).unwrap();
            async_io::block_on(recovery.checkpoint(
                crate::harness::CheckpointBoundary {
                    phase: crate::harness::CheckpointPhase::BeforeTool,
                    operation_id: Some(invocation),
                },
                &state,
            ))
            .unwrap();
            assert_eq!(
                crate::session_recovery::command_key("compiler-progress"),
                parent_key
            );
            let result =
                run_selected(&store, &gs, &["req:shop-1".into(), "req:shop-2".into()]).unwrap();
            assert_eq!(
                submissions.lock().unwrap().len(),
                before + 1,
                "one shared suite, not one run per requirement or criterion"
            );
            assert_eq!(result["runs"].as_array().unwrap().len(), 1);
            let ledger = Ledger::load(&out);
            let first = &ledger.requirements["req:shop-1"];
            assert_eq!(first.tests["crit:pass"].verdict, "pass");
            assert_eq!(first.tests["crit:fail"].verdict, "fail");
            assert_eq!(first.tests["crit:skip"].verdict, "none");
            let second = &ledger.requirements["req:shop-2"];
            assert_eq!(second.tests["crit:other"].verdict, "pass");
            assert_eq!(second.tests["crit:missing"].verdict, "none");
            assert_eq!(second.tests["crit:ambiguous"].verdict, "none");
            assert_ne!(status_of(&store, "req:shop-2", second, &gs).0, "verified");
            assert_eq!(
                serde_json::to_value(&ledger.requirements["req:unrelated"]).unwrap(),
                unrelated
            );
            let before_replay = submissions.lock().unwrap().len();
            let original = saved.lock().unwrap().as_ref().unwrap().0["parent"].clone();
            run_selected(&store, &gs, &["req:shop-1".into(), "req:shop-2".into()]).unwrap();
            assert_eq!(
                submissions.lock().unwrap().len(),
                before_replay,
                "same invocation reuses its destination receipt"
            );
            assert_eq!(
                saved.lock().unwrap().as_ref().unwrap().0["parent"],
                original,
                "same intention, destination run and deadline survive replay"
            );
            let requests = submissions.lock().unwrap();
            let submitted = requests.values().last().unwrap();
            let source = base64::engine::general_purpose::STANDARD
                .decode(submitted["files"]["src/cart.rs"].as_str().unwrap())
                .unwrap();
            assert_eq!(
                source,
                format!("// {owner}: revision {revision}\n").as_bytes()
            );
        }
        let (capsule, files) = saved.lock().unwrap().clone().unwrap();
        let before_restore = submissions.lock().unwrap().len();
        let _restored_fs = crate::vfs::Scope::enter(&files);
        let _restored_recovery = crate::session_recovery::Scope::enter(Some(&capsule));
        run_selected(&store, &gs, &["req:shop-1".into(), "req:shop-2".into()]).unwrap();
        assert_eq!(
            submissions.lock().unwrap().len(),
            before_restore,
            "restoring the same tool checkpoint does not dispatch another task"
        );
        assert_eq!(
            saved.lock().unwrap().as_ref().unwrap().0["parent"],
            capsule["parent"]
        );
    }
    let requests = submissions.lock().unwrap();
    assert_eq!(
        requests.len(),
        4,
        "two owners each run once against each of two source revisions"
    );
    let inputs: std::collections::BTreeSet<_> = requests
        .values()
        .map(|request| request["input_digest"].as_str().unwrap())
        .collect();
    assert_eq!(inputs.len(), 4, "every pass uses its own frozen input");
}

#[test]
fn task_restart_reuses_destination_intention_run_receipt_and_original_deadline() {
    for lose_reply in [false, true] {
        let mut initial = workspace("Tasks").snapshot();
        initial
            .get_mut(".jazyk/jazyk.toml")
            .unwrap()
            .extend_from_slice(
            b"\n[[tasks]]\nid='check'\nname='Check'\ndescription='Check source'\ncommand='true'\n",
        );
        let saved = Arc::new(std::sync::Mutex::new(None::<(Value, Snapshot)>));
        let effects = Arc::new(std::sync::Mutex::new(BTreeMap::<String, Value>::new()));
        let requests = Arc::new(AtomicUsize::new(0));
        let runtime_effects = effects.clone();
        let runtime_requests = requests.clone();
        let runtime: RuntimeTransport =
            Arc::new(
                move |request| match request["operation"].as_str().unwrap() {
                    "task_context" => Ok(json!({"available":true})),
                    "run_task" => {
                        let n = runtime_requests.fetch_add(1, Ordering::SeqCst);
                        let key = request["operation_key"].as_str().unwrap().to_owned();
                        let mut effects = runtime_effects.lock().unwrap();
                        if let Some(previous) = effects.get(&key) {
                            assert_eq!(previous, request);
                        }
                        effects.insert(key, request.clone());
                        if lose_reply && n == 0 {
                            Err("reply lost after destination accepted the task".into())
                        } else {
                            Ok(json!({"id":"original-run"}))
                        }
                    }
                    "wait_run" => {
                        assert_eq!(request["run_id"], "original-run");
                        Ok(json!({"terminated":true}))
                    }
                    "result" => {
                        let input = runtime_effects.lock().unwrap().values().next().unwrap()
                            ["files"]
                            .clone();
                        Ok(
                            json!({"status":"succeeded","complete":true,"input_files":input,
                    "log":"done","files":{},"reports":{},"cache":{},"deleted":[],"products":[]}),
                        )
                    }
                    operation => panic!("Unexpected task operation {operation}"),
                },
            );
        let checkpoint = saved.clone();
        let failed = Arc::new(AtomicBool::new(false));
        let fail = failed.clone();
        let sink: CheckpointSink = Arc::new(move |mut capsule, files| {
            let checkpoint = checkpoint.clone();
            let fail = fail.clone();
            Box::pin(async move {
                let next = capsule["sequence"].as_u64().unwrap() + 1;
                capsule["sequence"] = json!(next);
                let has_run = capsule["parent"]
                    .as_object()
                    .unwrap()
                    .values()
                    .any(|p| p["run_id"] == "original-run");
                *checkpoint.lock().unwrap() = Some((capsule, files));
                if has_run && !lose_reply && !fail.swap(true, Ordering::SeqCst) {
                    Err("checkpoint-failed: process lost after the run receipt was saved".into())
                } else {
                    Ok(next)
                }
            })
        });
        {
            let _fs = crate::vfs::Scope::enter(&initial);
            let _state = crate::scoped_state::Scope::enter();
            let _execution = ExecutionScope::enter(Execution {
                runtime: Some(runtime.clone()),
                checkpoint: Some(sink),
                ..Default::default()
            });
            let _recovery = crate::session_recovery::Scope::enter(None);
            let error =
                crate::task_runtime::execute_with_key(&root(), "check", "verify").unwrap_err();
            assert!(crate::session_recovery::is_interruption(&error), "{error}");
        }
        let (capsule, files) = saved.lock().unwrap().clone().unwrap();
        let deadline = capsule["parent"]
            .as_object()
            .unwrap()
            .values()
            .find_map(|p| p["deadline"].as_u64())
            .unwrap();
        {
            let _fs = crate::vfs::Scope::enter(&files);
            let _state = crate::scoped_state::Scope::enter();
            let _execution = ExecutionScope::enter(Execution {
                runtime: Some(runtime),
                resume: Some(capsule.clone()),
                ..Default::default()
            });
            let _recovery = crate::session_recovery::Scope::enter(Some(&capsule));
            let (id, result) =
                crate::task_runtime::execute_with_key(&root(), "check", "verify").unwrap();
            assert_eq!(id, "original-run");
            assert_eq!(result["status"], "succeeded");
            let key = capsule["parent"]
                .as_object()
                .unwrap()
                .iter()
                .find(|(_, value)| value["deadline"].as_u64().is_some())
                .unwrap()
                .0;
            assert_eq!(
                crate::session_recovery::parent(key).unwrap().unwrap()["deadline"],
                deadline
            );
        }
        assert_eq!(effects.lock().unwrap().len(), 1);
        assert_eq!(
            requests.load(Ordering::SeqCst),
            if lose_reply { 2 } else { 1 }
        );
    }
}
