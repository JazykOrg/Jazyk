// Durable execution: Stop, checkpoints, and resumed builds and chats.
use super::*;

#[test]
fn stop_discards_staged_batch_and_releases_only_saved_claims_after_partial_commit() {
    let mut workspace = workspace("Stop");
    workspace.files.insert(
        "docs/committed.txt".into(),
        b"An earlier accepted edit.\n".to_vec(),
    );
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let model_calls = calls.clone();
    let model: ModelAsyncTransport = Arc::new(move |_| {
        model_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(json!({"choices":[{"message":{"role":"assistant","tool_calls":[
            {"id":"stage","type":"function","function":{"name":"set_coverage","arguments":json!({
                "section":"docs/main.md#/stop","state":"non-normative","note":"Staged coverage, not committed."}).to_string()}}
        ]}}]}).to_string())
        })
    });
    let saved = Arc::new(std::sync::Mutex::new(None::<(Value, Snapshot)>));
    let saved_sink = saved.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    let sink: CheckpointSink = Arc::new(move |capsule, files| {
        let saved = saved_sink.clone();
        let stop = stop.clone();
        Box::pin(async move {
            let after_tool = capsule["frames"]
                .as_array()
                .and_then(|v| v.last())
                .is_some_and(|frame| frame["boundary"]["phase"] == "after_tool");
            if after_tool {
                *saved.lock().unwrap() = Some((capsule, files));
                stop.store(true, Ordering::SeqCst);
                Err("checkpoint-failed: cancellation raced with durable save".into())
            } else {
                Ok(capsule["sequence"].as_u64().unwrap() + 1)
            }
        })
    });
    let execution = Execution {
        model: "fixture".into(),
        transport_async: Some(model),
        checkpoint: Some(sink),
        cancel,
        ..Default::default()
    };
    let reply = workspace
        .request(
            "POST",
            "/api/jobs",
            &json!({"kind":"compile"}).to_string(),
            &execution,
        )
        .unwrap();
    assert!(
        reply.status < 400,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    assert!(
        !workspace
            .snapshot()
            .keys()
            .any(|path| path.contains("/leases/")),
        "active cancellation retained a claim"
    );
    let (capsule, files) = saved
        .lock()
        .unwrap()
        .clone()
        .expect("staged tool checkpoint");
    assert!(
        capsule["frames"][0]["mcp"]["open"]["session"]["staged"]
            .as_array()
            .is_some_and(|v| !v.is_empty()),
        "{capsule}"
    );
    assert!(files
        .keys()
        .any(|path| path.ends_with("/leases/build.yaml")));
    assert!(files.keys().any(|path| path.contains("/leases/b")));
    assert!(capsule["claims"].as_array().unwrap().len() >= 2);
    let mut recovered = Workspace::new(files.clone()).unwrap();
    recovered.stop_recovery(&capsule, &[]).unwrap();
    let stopped = recovered.snapshot();
    assert!(!stopped.keys().any(|path| path.contains("/leases/")));
    assert_eq!(
        stopped["docs/committed.txt"],
        b"An earlier accepted edit.\n"
    );
    for (path, bytes) in &files {
        if !path.contains("/leases/") && !path.starts_with(".jazyk/local/records/") {
            assert_eq!(
                stopped.get(path),
                Some(bytes),
                "Stop changed committed bytes: {path}"
            );
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1, "Stop invoked the model");
    let _fs = crate::vfs::Scope::enter(&stopped);
    let project = crate::document_sets::Workspace::load(&root())
        .unwrap()
        .resolve(Some("ds_main"))
        .unwrap();
    let guard = crate::control::begin_internal_build(&project, &project.out, "compile")
        .expect("fresh build must not inherit the stopped batch lease");
    drop(guard);
}

#[test]
fn actual_compiler_resume_hands_over_saved_claims_and_refuses_foreign_replacements() {
    let mut workspace = workspace("Alpha");
    let calls = Arc::new(AtomicUsize::new(0));
    let mut execution = navigation_model("Alpha", calls.clone(), None);
    let saved = Arc::new(std::sync::Mutex::new(None::<(Value, Snapshot)>));
    let save = saved.clone();
    let suspend = execution.suspend.clone();
    execution.checkpoint = Some(Arc::new(move |mut capsule, files| {
        let save = save.clone();
        let suspend = suspend.clone();
        Box::pin(async move {
            let next = capsule["sequence"].as_u64().unwrap() + 1;
            capsule["sequence"] = json!(next);
            if capsule["frames"]
                .as_array()
                .and_then(|frames| frames.last())
                .is_some_and(|frame| frame["boundary"]["phase"] == "after_tool")
            {
                suspend.store(true, Ordering::SeqCst);
            }
            *save.lock().unwrap() = Some((capsule, files));
            Ok(next)
        })
    }));
    let stopped = value(
        &mut workspace,
        "POST",
        "/api/jobs",
        json!({"kind":"compile","setIds":["ds_main"]}),
        &execution,
    );
    assert!(stopped.to_string().contains("suspended"), "{stopped}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let (mut capsule, mut files) = saved.lock().unwrap().clone().unwrap();
    let original_token = capsule["claims"]
        .as_array()
        .unwrap()
        .iter()
        .find(|claim| claim["lease"]["task"] == "build")
        .unwrap()["lease"]["worker"]
        .as_str()
        .unwrap()
        .to_owned();
    // Emulate a previous OS worker. Its protected receipts and VFS agree.
    // The replacement must acquire a fresh build identity and use it for its batch.
    for claim in capsule["claims"].as_array_mut().unwrap() {
        claim["lease"]["worker"] = json!("previous-process");
    }
    for (path, bytes) in files
        .iter_mut()
        .filter(|(path, _)| path.contains("/leases/"))
    {
        let mut lease: crate::control::Lease = serde_norway::from_slice(bytes).unwrap();
        lease.worker = "previous-process".into();
        *bytes = serde_norway::to_string(&lease).unwrap().into_bytes();
        assert!(!path.is_empty());
    }
    let mut foreign_files = files.clone();
    let (foreign_path, bytes) = foreign_files
        .iter_mut()
        .find(|(path, _)| path.contains("/leases/") && !path.ends_with("/build.yaml"))
        .unwrap();
    let foreign_path = foreign_path.clone();
    let mut lease: crate::control::Lease = serde_norway::from_slice(bytes).unwrap();
    lease.worker = "another-execution".into();
    *bytes = serde_norway::to_string(&lease).unwrap().into_bytes();
    let foreign_bytes = bytes.clone();
    let resume = Execution {
        resume: Some(capsule.clone()),
        ..navigation_model("Alpha", calls.clone(), None)
    };
    let mut foreign = Workspace::new(foreign_files).unwrap();
    let refused = value(
        &mut foreign,
        "POST",
        "/api/jobs",
        json!({"kind":"compile","setIds":["ds_main"]}),
        &resume,
    );
    assert!(refused.to_string().contains("incomplete"), "{refused}");
    assert_eq!(foreign.snapshot()[&foreign_path], foreign_bytes);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "Foreign claim dispatched inference"
    );
    let observed = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let observe = observed.clone();
    let checkpoint: CheckpointSink = Arc::new(move |capsule, _| {
        let observe = observe.clone();
        Box::pin(async move {
            let next = capsule["sequence"].as_u64().unwrap() + 1;
            observe.lock().unwrap().push(capsule);
            Ok(next)
        })
    });
    let resume = Execution {
        checkpoint: Some(checkpoint),
        ..resume
    };
    let mut restored = Workspace::new(files).unwrap();
    let complete = value(
        &mut restored,
        "POST",
        "/api/jobs",
        json!({"kind":"compile","setIds":["ds_main"]}),
        &resume,
    );
    assert!(complete.to_string().contains("converged"), "{complete}");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "Saved model response was billed again"
    );
    assert!(!restored
        .snapshot()
        .keys()
        .any(|path| path.contains("/leases/")));
    assert!(
        observed
            .lock()
            .unwrap()
            .iter()
            .any(
                |capsule| capsule["claims"].as_array().is_some_and(|claims| {
                    let Some(token) = claims
                        .iter()
                        .find(|claim| claim["lease"]["task"] == "build")
                        .and_then(|claim| claim["lease"]["worker"].as_str())
                    else {
                        return false;
                    };
                    !token.is_empty()
                        && token != original_token
                        && token != "previous-process"
                        && token != "another-execution"
                        && claims.iter().any(|claim| {
                            claim["lease"]["task"] != "build" && claim["lease"]["worker"] == token
                        })
                })
            ),
        "Restored MCP batch was not reclaimed under the replacement's fresh build token"
    );
}

#[test]
fn stopped_before_inference_chat_sends_the_next_human_instructions_in_full() {
    let mut w = workspace("Stopped before inference");
    let chat = value(
        &mut w,
        "POST",
        "/api/chat/sessions",
        json!({}),
        &Execution::default(),
    );
    let path = format!("/api/chat/sessions/{}/prompt", chat["id"].as_str().unwrap());
    let saved = Arc::new(std::sync::Mutex::new(None::<(Value, Snapshot)>));
    let checkpoint = saved.clone();
    let sink: CheckpointSink = Arc::new(move |mut capsule, files| {
        let checkpoint = checkpoint.clone();
        Box::pin(async move {
            let next = capsule["sequence"].as_u64().unwrap() + 1;
            capsule["sequence"] = json!(next);
            let before_model = capsule["frames"]
                .as_array()
                .and_then(|frames| frames.last())
                .is_some_and(|frame| frame["boundary"]["phase"] == "before_model");
            *checkpoint.lock().unwrap() = Some((capsule, files));
            if before_model {
                Err("checkpoint-failed: injected loss before inference".into())
            } else {
                Ok(next)
            }
        })
    });
    let execution = Execution {
        checkpoint: Some(sink),
        model: "must-not-run".into(),
        transport: Some(Arc::new(|_| {
            panic!("A protected pre-inference Stop must not invoke a model")
        })),
        ..Default::default()
    };
    let stopped = value(
        &mut w,
        "POST",
        &path,
        json!({"text":"Create the recovery document.","messageId":"original"}),
        &execution,
    );
    assert_eq!(stopped["stop"], "suspended");
    let (capsule, files) = saved.lock().unwrap().clone().unwrap();
    // The same Stop under a host record store restores the conversation there and
    // leaves no record file in the snapshot (docs/compiler/hosted-runtime.md#host-records).
    {
        use crate::hosted::records::{RecordStore, Stream};
        // The host already holds the conversation: move the snapshot's record files
        // into an in-memory host store.
        let host = Arc::new(super::MemoryRecords::default());
        let mut moved = files.clone();
        for (path, bytes) in &files {
            let Some(rest) = path.strip_prefix(".jazyk/local/records/") else {
                continue;
            };
            let stream = match rest.split('/').next() {
                Some("chat-updates") => Stream::ChatUpdates,
                Some("chat-history") => Stream::ChatHistory,
                _ => continue,
            };
            let doc: Value = serde_json::from_slice(bytes).unwrap();
            let key = doc["key"].as_str().unwrap();
            host.put_meta(stream, key, &doc["meta"]).unwrap();
            for item in doc["items"].as_array().unwrap() {
                host.append(
                    stream,
                    key,
                    0,
                    item[0].as_u64().unwrap(),
                    &[item[1].clone()],
                )
                .unwrap();
            }
            moved.remove(path);
        }
        let mut in_host = Workspace::new(moved).unwrap();
        in_host
            .stop_recovery_with(
                &capsule,
                &[],
                &Execution {
                    records: Some(host.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(
            !in_host
                .snapshot()
                .keys()
                .any(|p| p.starts_with(".jazyk/local/records/")),
            "Stop wrote records into the snapshot"
        );
        let id = chat["id"].as_str().unwrap();
        let meta = host.meta(Stream::ChatUpdates, id).unwrap().unwrap();
        assert_eq!(meta["state"], "idle", "{meta}");
        let updates = host
            .read(Stream::ChatUpdates, id, None, usize::MAX)
            .unwrap();
        assert!(
            updates
                .iter()
                .any(|(_, u)| u["sessionUpdate"] == "turn_end"),
            "{updates:?}"
        );
    }
    let mut stopped = Workspace::new(files).unwrap();
    stopped.stop_recovery(&capsule, &[]).unwrap();
    let mut stopped = Workspace::new(stopped.snapshot()).unwrap();
    let requests = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let observed = requests.clone();
    let execution = Execution {
        model: "followup-fixture".into(),
        transport: Some(Arc::new(move |body| {
            observed
                .lock()
                .unwrap()
                .push(serde_json::from_str(body).unwrap());
            Ok(model_reply(
                "The earlier request was to create the recovery document.",
            ))
        })),
        ..Default::default()
    };
    let instruction = "Do not write any files. Just explain the earlier task.";
    value(
        &mut stopped,
        "POST",
        &path,
        json!({"text":instruction,"messageId":"followup"}),
        &execution,
    );
    let requests = requests.lock().unwrap();
    assert_eq!(
        requests.len(),
        1,
        "No stopped inference or tool may be replayed"
    );
    let humans = requests[0]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "user")
        .filter_map(|message| message["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(humans.contains("Create the recovery document."), "{humans}");
    assert!(
        humans.contains(instruction),
        "New instructions vanished from actual provider input: {humans}"
    );
    assert!(!stopped.snapshot().contains_key("docs/recovery.md"));
}

#[test]
fn actual_gdk_chat_restores_saved_response_tool_effect_and_finished_turn() {
    for phase in ["saved_model", "before_tool", "after_tool", "finished"] {
        for response_kind in ["completion", "responses"] {
            let mut w = workspace("Recovery");
            let chat = value(
                &mut w,
                "POST",
                "/api/chat/sessions",
                json!({}),
                &Execution::default(),
            );
            let path = format!("/api/chat/sessions/{}/prompt", chat["id"].as_str().unwrap());
            let body = json!({"text":"Create the recovery document.","messageId":"accepted-1","turnId":"turn-1"});
            let calls = Arc::new(AtomicUsize::new(0));
            let model_calls = calls.clone();
            let model: ModelAsyncTransport = Arc::new(move |request| {
                let model_calls = model_calls.clone();
                Box::pin(async move {
                    let request: Value = serde_json::from_str(&request).unwrap();
                    assert!(request["_jazyk"]["requestId"].is_string());
                    let n = model_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(if n == 0 && response_kind == "responses" {
                        json!({"object":"response","id":"response-fixture","model":"fixture","status":"completed",
                        "_jazykResponseRoute":"fixture-route","output":[
                            {"id":"reasoning-fixture","type":"reasoning","summary":[],"encrypted_content":"opaque-reasoning-fixture"},
                            {"id":"call-fixture","type":"function_call","call_id":"write-1","name":"write_document","status":"completed",
                                "arguments":json!({"path":"docs/recovery.md","text":"# Recovered\n\nSaved once.\n"}).to_string()}],
                        "usage":{"input_tokens":10,"output_tokens":3,"total_tokens":13}}).to_string()
                    } else if n == 0 {
                        json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"write-1","type":"function","function":{"name":"write_document","arguments":json!({"path":"docs/recovery.md","text":"# Recovered\n\nSaved once.\n"}).to_string()}}]}}],"usage":{"total_tokens":13,"prompt_tokens":10,"completion_tokens":3}}).to_string()
                    } else {
                        assert_eq!(n, 1, "A completed provider call was replayed at {phase}");
                        model_reply("Recovery complete.")
                    })
                })
            });
            let saved = Arc::new(std::sync::Mutex::new(None::<(Value, Snapshot)>));
            let checkpoint = saved.clone();
            let injected = Arc::new(AtomicBool::new(false));
            let inject = injected.clone();
            let sink: CheckpointSink = Arc::new(move |mut capsule, files| {
                let checkpoint = checkpoint.clone();
                let inject = inject.clone();
                Box::pin(async move {
                    let next = capsule["sequence"].as_u64().unwrap() + 1;
                    capsule["sequence"] = json!(next);
                    let current_phase = capsule["frames"]
                        .as_array()
                        .and_then(|frames| frames.last())
                        .and_then(|frame| frame["boundary"]["phase"].as_str());
                    let fail = current_phase == Some(phase) && !inject.swap(true, Ordering::SeqCst);
                    *checkpoint.lock().unwrap() = Some((capsule, files));
                    if fail {
                        Err("checkpoint-failed: injected process loss after durable save".into())
                    } else {
                        Ok(next)
                    }
                })
            });
            let execution = Execution {
                model: "fixture".into(),
                transport_async: Some(model.clone()),
                checkpoint: Some(sink),
                ..Default::default()
            };
            let stopped = value(&mut w, "POST", &path, body.clone(), &execution);
            assert_eq!(stopped["stop"], "suspended", "{phase}: {stopped}");
            let (capsule, files) = saved.lock().unwrap().clone().unwrap();
            crate::session_recovery::validate(&capsule).unwrap();
            let mut normalized: crate::harness::HarnessState =
                serde_json::from_value(capsule["frames"][0]["harness"].clone()).unwrap();
            let stopped_history = crate::harness::stopped_history(&normalized);
            normalized.conversation =
                goose_provider_types::conversation::Conversation::new_unvalidated(
                    stopped_history
                        .iter()
                        .filter_map(|message| message.get("_jazykGdk"))
                        .map(|message| serde_json::from_value(message.clone()).unwrap())
                        .collect::<Vec<_>>(),
                );
            assert_eq!(
                crate::harness::stopped_history(&normalized),
                stopped_history,
                "Normalizing an already stopped transcript duplicated evidence at {phase}"
            );
            if response_kind == "responses" {
                assert!(
                    stopped_history
                        .iter()
                        .any(|message| message.to_string().contains("opaque-reasoning-fixture")),
                    "Stopped transcript lost native Responses context at {phase}"
                );
            }
            assert!(capsule["deliveredInputs"]
                .as_array()
                .unwrap()
                .contains(&json!("accepted-1")));
            let committed = matches!(phase, "after_tool" | "finished");
            assert_eq!(files.contains_key("docs/recovery.md"), committed);
            let mut cancelled = Workspace::new(files.clone()).unwrap();
            let prior = value(
                &mut cancelled,
                "GET",
                &path.replace("/prompt", ""),
                Value::Null,
                &Execution::default(),
            );
            let next = prior["updates"]
                .as_array()
                .unwrap()
                .last()
                .map_or(0, |u| u["n"].as_u64().unwrap() + 1);
            let retained = json!({"type":"chat.update","sessionId":chat["id"],"n":next,
            "update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Retained partial response."}}});
            cancelled
                .stop_recovery(&capsule, &[retained.clone(), retained])
                .unwrap();
            let cancelled_history = value(
                &mut cancelled,
                "GET",
                &path.replace("/prompt", ""),
                Value::Null,
                &Execution::default(),
            );
            assert_eq!(
                cancelled_history["state"], "idle",
                "{phase}: {cancelled_history}"
            );
            let cancelled_updates = cancelled_history["updates"].as_array().unwrap();
            assert_eq!(
                cancelled_updates
                    .iter()
                    .filter(|u| u["update"]["content"]["text"] == "Retained partial response.")
                    .count(),
                1,
                "{phase}"
            );
            assert_eq!(
                cancelled_updates.last().unwrap()["n"],
                json!(next + 1),
                "{phase}"
            );
            assert_eq!(
                cancelled_updates
                    .iter()
                    .filter(|u| u["update"]["sessionUpdate"] == "turn_end"
                        && u["update"]["stop"] == "cancelled")
                    .count(),
                1,
                "{phase}"
            );
            let requests = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
            let observed = requests.clone();
            let followup = Execution {
                model: "followup-fixture".into(),
                transport: Some(Arc::new(move |body| {
                    observed
                        .lock()
                        .unwrap()
                        .push(serde_json::from_str(body).unwrap());
                    Ok(model_reply(
                        "The prior request was to create the recovery document.",
                    ))
                })),
                ..Default::default()
            };
            value(
                &mut cancelled,
                "POST",
                &path,
                json!({"text":"try again","messageId":"followup-1","turnId":"turn-2"}),
                &followup,
            );
            let requests = requests.lock().unwrap();
            assert_eq!(
                requests.len(),
                1,
                "Stop or followup replayed inference at {phase}"
            );
            let messages = requests[0]["messages"].as_array().unwrap();
            assert!(
                messages.iter().any(|message| message["role"] == "user"
                    && message["content"]
                        .as_str()
                        .is_some_and(|text| text.contains("Create the recovery document."))),
                "Stopped chat lost the accepted human request at {phase}: {}",
                requests[0]
            );
            {
                // The proposal is the run's first inference. Its receipt may already
                // have left the capsule with its transition; its identity has not.
                let inference = format!(
                    "{}:0:Inference:0",
                    capsule["frames"][0]["harness"]["run_id"].as_str().unwrap()
                );
                let call_id = format!("{}:write-1", &crate::model::hash_hex(&inference)[..16]);
                assert!(
                    messages.iter().any(|message| message["role"] == "assistant"
                        && message["tool_calls"]
                            .as_array()
                            .is_some_and(|calls| calls.iter().any(|call| call["id"] == call_id))),
                    "Stopped chat lost its assistant tool request at {phase}: {messages:?}"
                );
                assert!(
                    messages
                        .iter()
                        .any(|message| message["role"] == "tool"
                            && message["tool_call_id"] == call_id),
                    "Stopped chat lost the completed tool receipt at {phase}"
                );
                if !committed {
                    assert!(
                        messages.iter().any(|message| message["role"] == "tool"
                            && message["content"]
                                .as_str()
                                .is_some_and(|content| content.contains("interrupted"))),
                        "An undispatched proposal needs an interrupted result"
                    );
                }
            }
            assert_eq!(
                cancelled.snapshot().contains_key("docs/recovery.md"),
                committed,
                "Following up dispatched a saved proposal at {phase}"
            );
            let mut restored = Workspace::new(files).unwrap();
            let resumed = Execution {
                model: "fixture".into(),
                transport_async: Some(model),
                resume: Some(capsule),
                ..Default::default()
            };
            let complete = value(&mut restored, "POST", &path, body, &resumed);
            assert_eq!(complete["stop"], "end_turn", "{phase}: {complete}");
            assert_eq!(calls.load(Ordering::SeqCst), 2, "{phase}");
            assert_eq!(
                restored.snapshot()["docs/recovery.md"],
                b"# Recovered\n\nSaved once.\n"
            );
            let timeline = value(
                &mut restored,
                "GET",
                &path.replace("/prompt", ""),
                Value::Null,
                &Execution::default(),
            );
            let updates = timeline["updates"].as_array().unwrap();
            assert_eq!(
                updates
                    .iter()
                    .filter(|u| u["update"]["sessionUpdate"] == "user_message")
                    .count(),
                1,
                "{phase}"
            );
            assert_eq!(
                updates
                    .iter()
                    .filter(|u| u["update"]["sessionUpdate"] == "turn_end")
                    .count(),
                1,
                "{phase}"
            );
        }
    }
}

#[test]
fn old_amnesic_chat_recovers_its_own_timeline_in_the_next_provider_request_once() {
    for already_amnesic in [false, true] {
        let mut w = workspace("Legacy chat");
        value(
            &mut w,
            "POST",
            "/api/chat/sessions",
            json!({}),
            &Execution::default(),
        );
        value(
            &mut w,
            "POST",
            "/api/chat/sessions",
            json!({}),
            &Execution::default(),
        );
        let mut book = stored_book(&w.files);
        let chat = &mut book["chats"]["chat-1"];
        chat["state"] = json!("idle");
        chat["titled"] = json!(true);
        chat["history"] = if already_amnesic {
            json!([
                {"role":"user","content":"try again"},
                {"role":"assistant","content":"I do not have the earlier request."}
            ])
        } else {
            json!([])
        };
        chat["updates"] = json!([
            [0,{"sessionUpdate":"user_message","messageId":"original","text":"Restyle the UML diagram in deliverable/index.html"}],
            [1,{"sessionUpdate":"agent_message_chunk","content":{"text":"Updating UML cards."}}],
            [2,{"sessionUpdate":"tool_call","toolCallId":"write","title":"write_text_file","rawInput":{"path":"deliverable/index.html"}}],
            [3,{"sessionUpdate":"tool_call_update","toolCallId":"write","status":"completed","rawOutput":"Saved deliverable/index.html"}],
            [4,{"sessionUpdate":"turn_end","stop":"cancelled"}],
            [5,{"sessionUpdate":"user_message","messageId":"retry","text":"try again"}],
            [6,{"sessionUpdate":"agent_message_chunk","content":{"text":"I do not have the earlier request."}}],
            [7,{"sessionUpdate":"turn_end","stop":"end_turn"}]
        ]);
        if !already_amnesic {
            chat["updates"].as_array_mut().unwrap().truncate(5);
        }
        chat["next_n"] = json!(8);
        book["chats"]["chat-2"]["updates"] = json!([
            [0,{"sessionUpdate":"user_message","text":"DO_NOT_CROSS_OTHER_CHAT"}]
        ]);
        w.files.insert(
            ".jazyk/conversations.json".into(),
            serde_json::to_vec(&book).unwrap(),
        );
        w.files.insert(
            "deliverable/index.html".into(),
            b"Previously saved diagram".to_vec(),
        );
        let requests = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let observed = requests.clone();
        let execution = Execution {
            model: "legacy-repair-fixture".into(),
            transport: Some(Arc::new(move |body| {
                observed
                    .lock()
                    .unwrap()
                    .push(serde_json::from_str(body).unwrap());
                Ok(model_reply(
                    "The earlier request was to restyle the UML diagram.",
                ))
            })),
            ..Default::default()
        };
        value(
            &mut w,
            "POST",
            "/api/chat/sessions/chat-1/prompt",
            json!({"text":"try again","messageId":"new-retry"}),
            &execution,
        );
        let mut reopened = Workspace::new(w.snapshot()).unwrap();
        value(
            &mut reopened,
            "POST",
            "/api/chat/sessions/chat-1/prompt",
            json!({"text":"what was the task?","messageId":"later"}),
            &execution,
        );
        let requests = requests.lock().unwrap();
        assert_eq!(
            requests.len(),
            2,
            "History repair must not invoke extra inference"
        );
        for request in requests.iter() {
            let messages = request["messages"].as_array().unwrap();
            let receipts = messages
                .iter()
                .filter(|message| {
                    message["role"] == "user"
                        && message["content"].as_str().is_some_and(|text| {
                            text.starts_with("Earlier conversation evidence was recovered")
                        })
                })
                .collect::<Vec<_>>();
            assert_eq!(
                receipts.len(),
                1,
                "The receipt must survive reopening without duplication: {:?}",
                messages
                    .iter()
                    .filter(|message| message["role"] != "system")
                    .collect::<Vec<_>>()
            );
            let text = receipts[0]["content"].as_str().unwrap();
            assert!(text.contains("Restyle the UML diagram in deliverable/index.html"));
            assert!(text.contains("Updating UML cards."));
            assert!(text.contains("Saved deliverable/index.html"));
            assert!(
                !messages
                    .iter()
                    .any(|message| message["role"] == "tool"
                        || message.get("tool_calls").is_some()),
                "Historical observations must not become synthetic tool calls or outcomes"
            );
            assert!(!request.to_string().contains("DO_NOT_CROSS_OTHER_CHAT"));
        }
        assert_eq!(
            reopened.snapshot()["deliverable/index.html"],
            b"Previously saved diagram"
        );
    }
}

#[test]
fn owned_build_steering_recovers_cursor_and_preserves_conversation() {
    let mut w = workspace("Steering");
    let incoming = Arc::new(AtomicBool::new(false));
    let incoming_model = incoming.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_model = cancel.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_model = calls.clone();
    let events = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let sink = events.clone();
    let execution = Execution {
        model: "build-steering-fixture".into(),
        cancel,
        transport: Some(Arc::new(move |body| {
            if calls_model.fetch_add(1, Ordering::SeqCst) == 0 {
                incoming_model.store(true, Ordering::SeqCst);
                return Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{
                    "id":"stale-coverage","type":"function","function":{"name":"set_coverage",
                    "arguments":json!({"section":"docs/main.md#/steering","state":"non-normative","note":"stale proposal"}).to_string()}
                }]}}]}).to_string());
            }
            assert!(body.contains("Stop changing coverage. Inspect the source first."));
            assert!(body.contains("Skipped: new human input arrived before dispatch"));
            cancel_model.store(true, Ordering::SeqCst);
            Ok(model_reply("Stopped after receiving the correction."))
        })),
        inputs: Some(Arc::new(move |session| {
            assert_eq!(
                session, "chat-1",
                "the worker batch must drain its ordinary conversation"
            );
            if incoming.swap(false, Ordering::SeqCst) {
                vec![
                    json!({"sessionId":session,"messageId":"build-steer-1","text":"Stop changing coverage. Inspect the source first.","turnId":"41","sequence":1}),
                ]
            } else {
                vec![]
            }
        })),
        input_close: Some(Arc::new(|_| {
            panic!("a worker batch cannot close whole-job admission")
        })),
        events: Some(Arc::new(move |event| {
            sink.lock().unwrap().push(event.clone())
        })),
        ..Default::default()
    };
    let chat = value(&mut w, "POST", "/api/chat/sessions", json!({}), &execution);
    let id = chat["id"].as_str().unwrap();
    // A reopened workspace can lack updates already committed to the event DB.
    // Its empty conversation book is behind that DB's durable cursor of 800.
    let result = w.request_test("POST", "/api/jobs", &json!({"kind":"compile","setId":"ds_main","conversationId":id,"jobId":41,"sessions":1,"afterN":800}).to_string(), &execution).unwrap();
    assert_eq!(
        result.status,
        200,
        "{}",
        String::from_utf8_lossy(&result.body)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let timeline = value(
        &mut w,
        "GET",
        &format!("/api/chat/sessions/{id}"),
        Value::Null,
        &Execution::default(),
    );
    let rows = timeline["updates"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        2,
        "one workflow marker and one delivered human message"
    );
    assert_eq!(rows[0]["n"], 801);
    assert_eq!(rows[1]["n"], 802);
    assert!(rows
        .iter()
        .any(|v| v["update"]["sessionUpdate"] == "workflow" && v["update"]["jobId"] == 41));
    assert_eq!(
        rows.iter()
            .filter(|v| v["update"]["sessionUpdate"] == "user_message"
                && v["update"]["messageId"] == "build-steer-1")
            .count(),
        1
    );
    assert!(timeline["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["messageId"] == "build-steer-1" && v["state"] == "delivered"));
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .any(|v| v["type"] == "chat.input"
            && v["messageId"] == "build-steer-1"
            && v["state"] == "delivered"));
    let history = stored_book(&w.snapshot()).to_string();
    assert!(history.contains("Stop changing coverage. Inspect the source first."));
    assert!(!history.contains("stale proposal"));
    assert!(!w
        .snapshot()
        .values()
        .any(|v| String::from_utf8_lossy(v).contains("stale proposal")));
}

#[test]
fn owned_jobs_reject_unknown_conversation_before_execution() {
    let mut w = workspace("Ownership");
    let reply = w
        .request_test(
            "POST",
            "/api/jobs",
            r#"{"kind":"compile","conversationId":"missing","jobId":42}"#,
            &Execution::default(),
        )
        .unwrap();
    assert_eq!(reply.status, 404);
    let chat = value(
        &mut w,
        "POST",
        "/api/chat/sessions",
        json!({}),
        &Execution::default(),
    );
    let reply = w
        .request_test(
            "POST",
            "/api/jobs",
            &json!({"kind":"compile","conversationId":chat["id"]}).to_string(),
            &Execution::default(),
        )
        .unwrap();
    assert_eq!(reply.status, 400);
    let reply = w
        .request_test(
            "POST",
            "/api/jobs",
            &json!({"kind":"compile","conversationId":chat["id"],"jobId":42,"afterN":1u64 << 53})
                .to_string(),
            &Execution::default(),
        )
        .unwrap();
    assert_eq!(
        reply.status, 400,
        "the cursor must stay within safe JSON event integers"
    );
    assert!(!w.snapshot().keys().any(|k| k.contains("graph/")));
}

#[test]
fn stop_discards_staged_coverage_and_late_done_without_retry_then_build_resumes() {
    let mut w = workspace("Alpha");
    let e = navigation_model("Alpha", Arc::new(AtomicUsize::new(0)), None);
    value(&mut w, "POST", "/api/jobs", json!({"kind":"compile"}), &e);
    let baseline = w.snapshot();
    w.files.insert(
        "docs/main.md".into(),
        b"# Alpha\n\nUpdated navigation index.\n".to_vec(),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let stopping = Execution {
        cancel: cancel.clone(),
        model: "Alpha".into(),
        transport: Some(Arc::new(move |_| {
            let n = seen.fetch_add(1, Ordering::SeqCst);
            let actions = if n == 0 {
                vec![(
                    "set_coverage",
                    json!({"section":"docs/main.md#/alpha", "state":"non-normative", "note":"Navigation."}),
                )]
            } else {
                flag.store(true, Ordering::SeqCst);
                vec![
                    (
                        "mark_goal_done",
                        json!({"goal":"g:reconcile-section:docs/main.md#/alpha", "justification":"Done."}),
                    ),
                    ("done", json!({"summary":"late completion"})),
                ]
            };
            Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":actions.iter().enumerate().map(|(i,(name,args))| json!({"id":format!("call-{i}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()}}]}).to_string())
        })),
        ..Default::default()
    };
    value(
        &mut w,
        "POST",
        "/api/jobs",
        json!({"kind":"compile"}),
        &stopping,
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "stop must not retry or dispatch the late done"
    );
    for (path, bytes) in baseline.iter().filter(|(p, _)| p.contains("/journal/")) {
        assert_eq!(
            w.files.get(path),
            Some(bytes),
            "earlier commit changed: {path}"
        );
    }
    let stopped = value(
        &mut w,
        "GET",
        "/api/status",
        Value::Null,
        &Execution::default(),
    );
    assert_ne!(stopped["verdict"]["state"], "converged");
    let resumed = Arc::new(AtomicUsize::new(0));
    let e = navigation_model("Alpha", resumed.clone(), None);
    value(&mut w, "POST", "/api/jobs", json!({"kind":"compile"}), &e);
    assert!(
        resumed.load(Ordering::SeqCst) > 0,
        "staged coverage must have been discarded"
    );
    let before = resumed.load(Ordering::SeqCst);
    value(&mut w, "POST", "/api/jobs", json!({"kind":"compile"}), &e);
    assert_eq!(
        resumed.load(Ordering::SeqCst),
        before,
        "completed resume must be a no-op"
    );
}
