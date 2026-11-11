// Conversation input, model deltas, and the agent context of hosted chat.
use super::*;

#[test]
fn hosted_chat_receives_github_context_through_mcp_instructions() {
    let mut w = workspace("GitHub context");
    let e = Execution {
        model: "fixture".into(),
        runtime: Some(Arc::new(|request| {
            assert_eq!(request["operation"], "project_context");
            Ok(
                json!({"available":true,"location":"cloud","project":{"name":"Jazyk.org"},"github":{"url":"https://github.com/JazykOrg/jazyk-site","branch":"main","default_branch":"master"}}),
            )
        })),
        transport: Some(Arc::new(|body| {
            let body: Value = serde_json::from_str(body).unwrap();
            let messages = body["messages"].to_string();
            for fact in [
                "https://github.com/JazykOrg/jazyk-site",
                "default_branch",
                "master",
                "main",
                "Jazyk.org",
            ] {
                assert!(messages.contains(fact), "missing {fact}: {messages}");
            }
            assert!(body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["function"]["name"] == "project_context"));
            Ok(model_reply("The current branch is main."))
        })),
        ..Default::default()
    };
    let chat = value(&mut w, "POST", "/api/chat/sessions", json!({}), &e);
    value(
        &mut w,
        "POST",
        &format!("/api/chat/sessions/{}/prompt", chat["id"].as_str().unwrap()),
        json!({"messageId":"branch-question","text":"Which branch should the deploy workflow target?"}),
        &e,
    );
}

// Hosted chat reads the snapshot's instruction files and skills and the host's
// layer, never the host filesystem, and calls remote tools through the host.
// Mirrors docs/frontends/agent-context.md#hosted-context.
#[test]
fn hosted_chat_uses_the_exact_managed_policy_and_deferred_skill_tools() {
    let mut w = workspace("Shared workflows");
    w.sync_agents_md(false);
    let policy = {
        let _fs = crate::vfs::Scope::enter(&w.files);
        crate::agent_workflows::render(&crate::portable::root()).unwrap()
    };
    let before = w.snapshot();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let expected_skill = crate::agent_workflows::read("change-code").unwrap()["body"]
        .as_str()
        .unwrap()
        .to_string();
    let e = Execution {
        model: "fixture".into(),
        transport: Some(Arc::new(move |body| {
            let request: Value = serde_json::from_str(body).unwrap();
            let texts = request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|m| m["content"].as_str())
                .collect::<Vec<_>>();
            assert_eq!(
                texts
                    .iter()
                    .map(|t| t.matches(&policy).count())
                    .sum::<usize>(),
                1
            );
            for name in crate::agent_workflows::TOOLS {
                assert!(request["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t["function"]["name"] == name));
            }
            if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                return Ok(json!({"choices":[{"message":{"role":"assistant","content":null,
                "tool_calls":[{"id":"read-workflow","type":"function","function":{"name":"read_jazyk_skill","arguments":json!({"name":"change-code"}).to_string()}}]}}]}).to_string());
            }
            assert!(texts
                .iter()
                .filter_map(|t| serde_json::from_str::<Value>(t).ok())
                .any(|v| v["body"].as_str() == Some(expected_skill.as_str())));
            Ok(model_reply(
                "A repair stays within requirements. Changed behavior updates the docs first.",
            ))
        })),
        ..Default::default()
    };
    let chat = value(&mut w, "POST", "/api/chat/sessions", json!({}), &e);
    value(
        &mut w,
        "POST",
        &format!("/api/chat/sessions/{}/prompt", chat["id"].as_str().unwrap()),
        json!({"text":"Explain the workflow for a code change. Do not edit anything.","messageId":"workflow-question"}),
        &e,
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    for (path, bytes) in before {
        assert_eq!(
            w.snapshot().get(&path),
            Some(&bytes),
            "read-only skill request changed {path}"
        );
    }
    assert!(!w.snapshot().keys().any(|p| p.contains("graph/")));
}

#[test]
fn hosted_chat_receives_project_and_host_agent_context() {
    let mut w = workspace("Agent context");
    w.files
        .insert("AGENTS.md".into(), b"Prefer short sentences.\n".to_vec());
    w.files.insert(
        ".agents/skills/triage/SKILL.md".into(),
        b"---\nname: triage\ndescription: sort issues\n---\nSteps.".to_vec(),
    );
    w.files.insert(".mcp.json".into(), br#"{"mcpServers": {"web": {"url": "https://mcp.example.com/mcp"}, "local": {"command": "x"}}}"#.to_vec());
    let changed = w.sync_agents_md(true);
    assert!(
        changed
            .iter()
            .any(|c| c == "added the jazyk section to AGENTS.md"),
        "{changed:?}"
    );
    let agents = String::from_utf8(w.snapshot()["AGENTS.md"].clone()).unwrap();
    assert!(agents.starts_with("Prefer short sentences.\n\n<!-- JAZYK:BEGIN -->"));
    assert!(w.sync_agents_md(false).is_empty());
    let proposals = w.agent_mcp_proposals();
    assert_eq!(
        proposals
            .iter()
            .map(|p| (p["name"].as_str().unwrap(), p["hosted"].as_bool().unwrap()))
            .collect::<Vec<_>>(),
        vec![("local", false), ("web", true)]
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let host = crate::agent_context::HostContext {
        instructions: vec![("organization".into(), "Use British spelling.".into())],
        mcp: vec![crate::agent_context::RemoteServer {
            name: "issues".into(),
            instructions: None,
            tools: vec![crate::harness::GenericTool {
                name: "list_issues".into(),
                description: "List issues".into(),
                parameters: json!({"type": "object"}),
            }],
        }],
        call: Some(Arc::new(|server, tool, _| {
            Ok(format!("{server}:{tool}:42 open"))
        })),
        ..Default::default()
    };
    let e = Execution {
        model: "fixture".into(),
        agent_context: Some(Arc::new(host)),
        transport: Some(Arc::new(move |body| {
            let body: Value = serde_json::from_str(body).unwrap();
            let messages = body["messages"].to_string();
            let n = seen.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                for fact in [
                    "Prefer short sentences.",
                    "Use British spelling.",
                    "- triage: sort issues",
                    "precedence",
                ] {
                    assert!(messages.contains(fact), "missing {fact}: {messages}");
                }
                assert!(
                    !messages.contains("JAZYK:BEGIN"),
                    "the jazyk section stays out"
                );
                let names: Vec<&str> = body["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|t| t["function"]["name"].as_str())
                    .collect();
                assert!(
                    names.contains(&"read_skill") && names.contains(&"list_issues"),
                    "{names:?}"
                );
                return Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"r1","type":"function","function":{"name":"list_issues","arguments":"{}"}}]}}]}).to_string());
            }
            assert!(
                messages.contains("issues:list_issues:42 open"),
                "{messages}"
            );
            Ok(model_reply("42 issues are open."))
        })),
        ..Default::default()
    };
    let chat = value(&mut w, "POST", "/api/chat/sessions", json!({}), &e);
    value(
        &mut w,
        "POST",
        &format!("/api/chat/sessions/{}/prompt", chat["id"].as_str().unwrap()),
        json!({"messageId":"q","text":"How many issues are open?"}),
        &e,
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn hosted_input_callback_delivers_before_stale_write_and_persists_identity() {
    let mut w = workspace("Steering");
    w.files.insert(
        "docs/hosting.md".into(),
        b"# Hosting\n\nOriginal.\n".to_vec(),
    );
    let incoming = Arc::new(AtomicBool::new(false));
    let incoming_model = incoming.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let model_calls = calls.clone();
    let hash = crate::model::hash_hex("# Hosting\n\nOriginal.\n");
    let events = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let sink = events.clone();
    let e = Execution {
        model: "fixture".into(),
        transport: Some(Arc::new(move |body| {
            let n = model_calls.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                incoming_model.store(true, Ordering::SeqCst);
                return Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"stale-write","type":"function","function":{"name":"write_document","arguments":json!({"path":"docs/hosting.md","text":"must not land","baseHash":hash}).to_string()}}]}}]}).to_string());
            }
            assert!(body.contains("Do not change hosting.md"));
            Ok(model_reply("Kept hosting.md unchanged."))
        })),
        inputs: Some(Arc::new(move |session| {
            if incoming.swap(false, Ordering::SeqCst) {
                vec![
                    json!({"sessionId":session,"messageId":"steer-1","text":"Do not change hosting.md","sequence":2}),
                ]
            } else {
                vec![]
            }
        })),
        input_close: Some(Arc::new(|_| true)),
        events: Some(Arc::new(move |event| {
            sink.lock().unwrap().push(event.clone())
        })),
        ..Default::default()
    };
    let chat = value(&mut w, "POST", "/api/chat/sessions", json!({}), &e);
    value(
        &mut w,
        "POST",
        &format!("/api/chat/sessions/{}/prompt", chat["id"].as_str().unwrap()),
        json!({"messageId":"initial","text":"Update hosting.md"}),
        &e,
    );
    assert_eq!(w.snapshot()["docs/hosting.md"], b"# Hosting\n\nOriginal.\n");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let timeline = value(
        &mut w,
        "GET",
        &format!("/api/chat/sessions/{}", chat["id"].as_str().unwrap()),
        Value::Null,
        &e,
    );
    assert!(timeline["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["messageId"] == "steer-1" && v["state"] == "delivered"));
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .any(|v| v["type"] == "chat.input" && v["messageId"] == "steer-1"));
}

#[test]
fn chat_followup_edits_uncompiled_prose_without_lifecycle_work() {
    let mut w = workspace("Alpha");
    w.files.insert(
        "docs/hosting.md".into(),
        b"# Hosting\n\nCloudFront-S3.\n".to_vec(),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_model = calls.clone();
    let expected_hash = crate::model::hash_hex("# Hosting\n\nCloudFront-S3.\n");
    let updated = "# Hosting\n\nUse the existing deployment task.\n";
    let e = Execution {
        model: "fixture".into(),
        transport: Some(Arc::new(move |body| {
            let request: Value = serde_json::from_str(body).unwrap();
            let messages = request["messages"].as_array().unwrap();
            assert!(!messages.iter().any(|m| m["content"]
                .as_str()
                .is_some_and(|t| t.starts_with("Read task_context and choose"))));
            let n = calls_for_model.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                return Ok(model_reply(
                    "Credentials are sensitive. Resource identifiers are configuration.",
                ));
            }
            if n == 1 {
                return Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"write-hosting","type":"function","function":{"name":"write_document","arguments":json!({"path":"docs/hosting.md","text":updated,"baseHash":expected_hash}).to_string()}}]}}]}).to_string());
            }
            assert_eq!(n, 2, "unnecessary completion round");
            Ok(model_reply("Updated hosting.md."))
        })),
        ..Default::default()
    };
    let chat = value(&mut w, "POST", "/api/chat/sessions", json!({}), &e);
    let route = format!("/api/chat/sessions/{}/prompt", chat["id"].as_str().unwrap());
    value(
        &mut w,
        "POST",
        &route,
        json!({"text":"What credentials does a static deployment need?","messageId":"first"}),
        &e,
    );
    value(
        &mut w,
        "POST",
        &route,
        json!({"text":"Update the relevant section in hosting.md","messageId":"second"}),
        &e,
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(w.snapshot()["docs/hosting.md"], updated.as_bytes());
    let again = value(
        &mut w,
        "POST",
        &route,
        json!({"text":"Update the relevant section in hosting.md","messageId":"second"}),
        &e,
    );
    assert_eq!(again["messageId"], "second");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "retry re-executed a completed turn"
    );
    assert!(!w.snapshot().keys().any(|p| p.contains("graph/entity/")));
    let history = value(&mut w, "GET", "/api/chat/history", Value::Null, &e);
    let chat = &history["book"]["chats"][chat["id"].as_str().unwrap()];
    assert_eq!(chat["inputs"]["second"]["state"], "delivered");
    assert_eq!(chat["instruction_hash"].as_str().unwrap().len(), 16);
}

#[test]
fn restored_legacy_chat_replaces_harness_context_and_edits_only_named_document() {
    let mut w = workspace("Alpha");
    let original = "# Hosting\n\nCloudFront-S3.\n";
    let updated = "# Hosting\n\nUse the existing deployment task.\n";
    w.files
        .insert("docs/hosting.md".into(), original.as_bytes().to_vec());
    let e = Execution::default();
    let created = value(&mut w, "POST", "/api/chat/sessions", json!({}), &e);
    let id = created["id"].as_str().unwrap();
    let mut saved = value(&mut w, "GET", "/api/chat/history", Value::Null, &e);
    let legacy = include_str!("../../../fixtures/legacy-chat-context-v1.txt");
    let first = "What deployment credentials are needed?";
    let edit = "Update the relevant section in hosting.md.";
    let old_pair = json!({"role":"assistant","content":null,"reasoning_content":"Earlier worker deliberation remains provider history.","tool_calls":[{"id":"old-done","type":"function","function":{"name":"done","arguments":"{}"}}]});
    let old_result = json!({"role":"tool","tool_call_id":"old-done","content":"Committed b6-1. Next unclaimed batch b7-1."});
    let chat = &mut saved["book"]["chats"][id];
    chat["history"] = json!([
        {"role":"system","content":"Old generic harness"},
        {"role":"user","content":format!("{legacy}\n\n{first}")},old_pair.clone(),old_result.clone(),
        {"role":"user","content":format!("{legacy}\n\n{edit}")},
        {"role":"assistant","content":"Cancelled before the edit."}
    ]);
    chat["updates"] = json!([[0,{"sessionUpdate":"user_message","text":first}],[1,{"sessionUpdate":"user_message","text":edit}]]);
    chat["next_n"] = json!(2);
    chat["history_context_version"] = json!(0);
    value(&mut w, "PUT", "/api/chat/history", saved, &e);
    let requests = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let captured = requests.clone();
    let expected_hash = crate::model::hash_hex(original);
    let e = Execution {
        model: "fixture".into(),
        transport: Some(Arc::new(move |body| {
            let request: Value = serde_json::from_str(body).unwrap();
            let mut requests = captured.lock().unwrap();
            let round = requests.len();
            requests.push(request.clone());
            let messages = request["messages"].as_array().unwrap();
            let system = messages[0]["content"].as_str().unwrap();
            assert!(system.contains("historical evidence"));
            assert!(!messages.iter().any(|message| message["role"] == "user"
                && message["content"].as_str().is_some_and(
                    |text| text.starts_with("This project has independent document sets.")
                )));
            assert!(messages.contains(&old_pair));
            assert!(messages.contains(&old_result));
            if round == 0 {
                return Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"read","type":"function","function":{"name":"read_document","arguments":json!({"path":"docs/hosting.md"}).to_string()}}]}}]}).to_string());
            }
            if round == 1 {
                return Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"write","type":"function","function":{"name":"write_document","arguments":json!({"path":"docs/hosting.md","text":updated,"baseHash":expected_hash}).to_string()}}]}}]}).to_string());
            }
            assert_eq!(round, 2, "unexpected compiler or completion round");
            Ok(model_reply("Updated hosting.md."))
        })),
        ..Default::default()
    };
    value(
        &mut w,
        "POST",
        &format!("/api/chat/sessions/{id}/prompt"),
        json!({"text":"Try again. Do not compile; edit only hosting.md.","messageId":"retry-edit"}),
        &e,
    );
    assert_eq!(requests.lock().unwrap().len(), 3);
    assert_eq!(w.snapshot()["docs/hosting.md"], updated.as_bytes());
    assert!(!w
        .snapshot()
        .keys()
        .any(|path| path.contains("graph/entity/") || path.ends_with("control.yaml")));
    let saved = value(&mut w, "GET", "/api/chat/history", Value::Null, &e);
    let messages = saved["book"]["chats"][id]["history"].as_array().unwrap();
    // Current instructions are supplied separately to GDK, not persisted as
    // human or provider history.
    assert_eq!(
        messages
            .iter()
            .filter(|message| message["role"] == "system")
            .count(),
        0
    );
    assert_eq!(
        messages
            .iter()
            .filter(|message| message["role"] == "user")
            .map(|message| message["content"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            first,
            edit,
            "Try again. Do not compile; edit only hosting.md."
        ]
    );
}

#[test]
fn chat_histories_roundtrip_and_panics_restore_all_context() {
    let mut a = workspace("Alpha");
    let mut b = workspace("Beta");
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    let e = Execution {
        model: "fixture".into(),
        transport: Some(Arc::new(|_| Ok(model_reply("Hello from the fixture.")))),
        events: Some(Arc::new(move |v| sink.lock().unwrap().push(v.clone()))),
        ..Default::default()
    };
    let chat = value(&mut a, "POST", "/api/chat/sessions", json!({}), &e);
    value(
        &mut a,
        "POST",
        &format!("/api/chat/sessions/{}/prompt", chat["id"].as_str().unwrap()),
        json!({"text":"hello alpha"}),
        &e,
    );
    let mut restored = Workspace::new(a.snapshot()).unwrap();
    let list = value(&mut restored, "GET", "/api/chat/sessions", Value::Null, &e);
    assert_eq!(list["sessions"].as_array().unwrap().len(), 1);
    assert!(list.to_string().contains("hello alpha"));
    assert_eq!(
        value(&mut b, "GET", "/api/chat/sessions", Value::Null, &e)["sessions"],
        json!([])
    );
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .any(|v| v["type"] == "chat.update"));
    let before = restored.snapshot();
    let panic = Execution {
        transport: Some(Arc::new(|_| {
            crate::llm::add_tokens(999);
            panic!("test callback panic")
        })),
        ..Default::default()
    };
    assert!(restored
        .request_test(
            "POST",
            "/api/chat/sessions/chat-1/prompt",
            "{\"text\":\"panic\"}",
            &panic
        )
        .unwrap_err()
        .contains("rolled back"));
    assert_eq!(restored.snapshot(), before);
    assert!(!crate::vfs::is_scoped());
    let recovery_calls = AtomicUsize::new(0);
    let fresh = Execution {
        transport: Some(Arc::new(move |_| {
            if recovery_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                assert_eq!(crate::llm::tokens_spent(), 0);
            }
            Ok(model_reply("Recovered."))
        })),
        ..Default::default()
    };
    value(
        &mut restored,
        "POST",
        "/api/chat/sessions/chat-1/prompt",
        json!({"text":"recover"}),
        &fresh,
    );
}

#[test]
fn no_hosted_execution_has_no_pause_reason() {
    assert!(pause_reason().is_none());
}

#[test]
fn chat_provider_pause_keeps_its_reason_after_reopening() {
    let mut workspace = workspace("Pause");
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let pause_reason = Arc::new(std::sync::Mutex::new(None));
    let reason = pause_reason.clone();
    let execution = Execution {
        cancel,
        pause_reason,
        transport: Some(Arc::new(move |body| {
            let request: Value = serde_json::from_str(body).unwrap();
            let names: Vec<_> = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["function"]["name"].as_str().unwrap())
                .collect();
            for tool in [
                "write_text_file",
                "read_text_file",
                "list_files",
                "release_work",
            ] {
                assert!(names.contains(&tool), "{tool}: {names:?}");
            }
            assert!(!names.contains(&"run_command"));
            *reason.lock().unwrap() = Some("Provider deadline reached.".into());
            flag.store(true, Ordering::Relaxed);
            Err("Provider deadline reached.".into())
        })),
        ..Default::default()
    };
    value(
        &mut workspace,
        "POST",
        "/api/chat/sessions",
        json!({}),
        &execution,
    );
    value(
        &mut workspace,
        "POST",
        "/api/chat/sessions/chat-1/prompt",
        json!({"text":"Generate and test this project."}),
        &execution,
    );
    let mut restored = Workspace::new(workspace.snapshot()).unwrap();
    let chat = value(
        &mut restored,
        "GET",
        "/api/chat/sessions/chat-1",
        Value::Null,
        &Execution::default(),
    );
    let terminal = &chat["updates"].as_array().unwrap().last().unwrap()["update"];
    assert_eq!(terminal["stop"], "paused", "{chat}");
    assert_eq!(terminal["error"], "Provider deadline reached.");
}

// A version 4 snapshot kept every conversation in `.jazyk/conversations.json`. The first
// request reads it once, writes it through the record store, and removes the file, so
// the history survives the move. Mirrors docs/compiler/hosted-runtime.md#host-records.
#[test]
fn a_version_4_conversation_file_moves_into_the_record_store_once() {
    let mut w = workspace("Legacy book");
    let book = json!({"seq": 1, "chats": {"chat-1": {
        "id": "chat-1", "title": "Old talk", "titled": true, "state": "idle",
        "updates": [[0, {"sessionUpdate": "user_message", "text": "keep me"}]],
        "next_n": 1,
        "history": [{"role": "user", "content": "keep me"}],
        "reader": false
    }}});
    w.files.insert(
        ".jazyk/conversations.json".into(),
        serde_json::to_vec(&book).unwrap(),
    );
    let listed = value(
        &mut w,
        "GET",
        "/api/chat/sessions",
        json!({}),
        &Execution::default(),
    );
    assert!(listed.to_string().contains("Old talk"), "{listed}");
    let files = w.snapshot();
    assert!(!files.contains_key(".jazyk/conversations.json"));
    let stored = stored_book(&files);
    assert_eq!(stored["chats"]["chat-1"]["title"], "Old talk");
    assert_eq!(
        stored["chats"]["chat-1"]["history"][0]["content"],
        "keep me"
    );
    assert_eq!(
        stored["chats"]["chat-1"]["updates"][0][1]["text"],
        "keep me"
    );
}

// A host that supplies a record store keeps conversations there: nothing of the book
// lands in the workspace snapshot. Mirrors docs/compiler/hosted-runtime.md#host-records.
#[test]
fn a_host_record_store_keeps_conversations_out_of_the_snapshot() {
    use crate::hosted::records::{RecordStore, Stream};
    use std::sync::Mutex;
    #[derive(Default)]
    struct Memory(Mutex<BTreeMap<(String, String), (Value, Vec<(u64, Value)>)>>);
    impl RecordStore for Memory {
        fn keys(&self, s: Stream) -> Result<Vec<(String, Value)>, String> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|((st, _), _)| st == s.name())
                .map(|((_, k), (m, _))| (k.clone(), m.clone()))
                .collect())
        }
        fn meta(&self, s: Stream, k: &str) -> Result<Option<Value>, String> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(&(s.name().into(), k.into()))
                .map(|(m, _)| m.clone()))
        }
        fn put_meta(&self, s: Stream, k: &str, m: &Value) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .entry((s.name().into(), k.into()))
                .or_default()
                .0 = m.clone();
            Ok(())
        }
        fn append(
            &self,
            s: Stream,
            k: &str,
            _g: u32,
            from: u64,
            items: &[Value],
        ) -> Result<(), String> {
            let mut m = self.0.lock().unwrap();
            let e = m.entry((s.name().into(), k.into())).or_default();
            for (i, v) in items.iter().enumerate() {
                let n = from + i as u64;
                if !e.1.iter().any(|(x, _)| *x == n) {
                    e.1.push((n, v.clone()));
                }
            }
            Ok(())
        }
        fn replace(&self, s: Stream, k: &str, _g: u32, items: &[Value]) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .entry((s.name().into(), k.into()))
                .or_default()
                .1 = items
                .iter()
                .cloned()
                .enumerate()
                .map(|(n, v)| (n as u64, v))
                .collect();
            Ok(())
        }
        fn read(
            &self,
            s: Stream,
            k: &str,
            after: Option<u64>,
            limit: usize,
        ) -> Result<Vec<(u64, Value)>, String> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(&(s.name().into(), k.into()))
                .map(|(_, v)| {
                    v.iter()
                        .filter(|(n, _)| after.is_none_or(|a| *n > a))
                        .take(limit)
                        .cloned()
                        .collect()
                })
                .unwrap_or_default())
        }
        fn delete(&self, s: Stream, k: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(&(s.name().into(), k.into()));
            Ok(())
        }
    }
    let memory = Arc::new(Memory::default());
    let e = Execution {
        records: Some(memory.clone()),
        ..Default::default()
    };
    let mut w = workspace("Hosted records");
    value(&mut w, "POST", "/api/chat/sessions", json!({}), &e);
    assert!(!w
        .snapshot()
        .keys()
        .any(|p| p.starts_with(".jazyk/local/records/") || p.ends_with("conversations.json")));
    let keys = memory.keys(Stream::ChatUpdates).unwrap();
    assert!(keys.iter().any(|(k, _)| k == "chat-1"), "{keys:?}");
    let listed = value(&mut w, "GET", "/api/chat/sessions", Value::Null, &e);
    assert!(listed.to_string().contains("chat-1"), "{listed}");
}
