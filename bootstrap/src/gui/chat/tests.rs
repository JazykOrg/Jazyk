// The chat pane's handlers end to end against a scratch project: sessions open,
// prompt, queue inputs, cancel, delete, and come back after a restart; slash
// commands run against zero, one, and several document sets. No agent process is
// started: prompts that reach the agent name a profile that does not exist.
// Mirrors docs/frontends/gui.md#chat-input-admission and docs/frontends/acp.md#slash-commands.
use super::*;

const NO_AGENT: &str = "jazyk-test-no-such-agent";

fn state(tag: &str) -> SharedState {
    super::super::server::tests::state(None, &format!("https://chat-{}.test", tag))
}

// Rewrite the project's jazyk.toml and reload the project the handlers read.
fn configure(st: &SharedState, sets: &[(&str, &str)]) {
    let mut toml = format!("version = 1\n\n[acp]\nagent = \"{}\"\n", NO_AGENT);
    for (id, name) in sets {
        toml.push_str(&format!(
            "\n[[sets]]\nid = \"{id}\"\nname = \"{name}\"\ndocs = [\"{id}/**/*.md\"]\n"
        ));
    }
    std::fs::create_dir_all(st.root.join(".jazyk")).unwrap();
    std::fs::write(st.root.join(crate::document_sets::CONFIG), toml).unwrap();
    let mut proj = crate::project::Project::load(&st.root);
    proj.out = st.out.clone();
    *st.proj.write().unwrap() = proj;
}

async fn body(r: Response) -> (StatusCode, Value) {
    let status = r.status();
    let bytes = axum::body::to_bytes(r.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn open(st: &SharedState) -> String {
    let (status, v) = body(post_session(State(st.clone())).await).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v["id"].as_str().unwrap().to_string()
}

async fn prompt(st: &SharedState, id: &str, b: Value) -> (StatusCode, Value) {
    body(post_prompt(State(st.clone()), Path(id.to_string()), Json(b)).await).await
}

async fn input(st: &SharedState, id: &str, b: Value) -> (StatusCode, Value) {
    body(post_input(State(st.clone()), Path(id.to_string()), Json(b)).await).await
}

async fn get(st: &SharedState, id: &str) -> (StatusCode, Value) {
    body(get_session(State(st.clone()), Path(id.to_string())).await).await
}

fn session(st: &SharedState, id: &str) -> Arc<Mutex<ChatSession>> {
    st.chat.sessions.lock().unwrap().get(id).cloned().unwrap()
}

// The kinds of a session's updates, in order.
fn kinds(v: &Value) -> Vec<String> {
    v["updates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| {
            u["update"]["sessionUpdate"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

fn agent_text(v: &Value) -> String {
    v["updates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|u| u["update"]["sessionUpdate"] == "agent_message")
        .map(|u| u["update"]["text"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

// Wait for a session state the blocking turn thread reaches; the bound only turns a
// hang into a failure.
async fn settle(st: &SharedState, id: &str, want: &str) -> Value {
    for _ in 0..6000 {
        let (_, v) = get(st, id).await;
        if v["state"] == want {
            return v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("{id} never reached {want}: {:?}", get(st, id).await);
}

// A new conversation is numbered on from the last, listed with the command catalog,
// announced on the event stream, and recorded in the session store.
#[tokio::test]
async fn sessions_open_numbered_listed_announced_and_stored() {
    let st = state("open");
    configure(&st, &[("ds_main", "Main")]);
    let mut events = st.events.subscribe();
    assert_eq!(open(&st).await, "chat-1");
    assert_eq!(open(&st).await, "chat-2");
    let (status, list) = body(list_sessions(State(st.clone())).await).await;
    assert_eq!(status, StatusCode::OK);
    let sessions = list["sessions"].as_array().unwrap();
    assert_eq!(
        sessions.iter().map(|s| s["id"].clone()).collect::<Vec<_>>(),
        vec![json!("chat-1"), json!("chat-2")]
    );
    assert_eq!(sessions[0]["title"], "Chat 1");
    assert_eq!(sessions[0]["state"], "idle");
    let commands: Vec<&str> = list["commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["name"].as_str())
        .collect();
    assert!(
        commands.contains(&"/help") && commands.contains(&"/status"),
        "{commands:?}"
    );
    assert!(commands.iter().all(|c| c.starts_with('/')));
    let announced = events.try_recv().unwrap();
    assert_eq!(announced["type"], "chat.sessions");
    let stored: Vec<String> = crate::acp::sessions::list(&st.out)
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert!(
        stored.contains(&"chat-1".to_string()) && stored.contains(&"chat-2".to_string()),
        "{stored:?}"
    );
    std::fs::remove_dir_all(&st.root).ok();
}

// Unknown ids answer 404 on every handler; an empty prompt is a 400; an input
// needs both its id and its text.
#[tokio::test]
async fn unknown_sessions_and_empty_prompts_are_refused() {
    let st = state("refusals");
    configure(&st, &[("ds_main", "Main")]);
    let ghost = "chat-404";
    assert_eq!(get(&st, ghost).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        prompt(&st, ghost, json!({"text":"/help"})).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        input(&st, ghost, json!({"text":"x","messageId":"m"}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        cancel(State(st.clone()), Path(ghost.to_string()))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        delete_session(State(st.clone()), Path(ghost.to_string()))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let id = open(&st).await;
    assert_eq!(
        prompt(&st, &id, json!({"text":"   "})).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        input(&st, &id, json!({"text":"hello"})).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        input(&st, &id, json!({"messageId":"m1"})).await.0,
        StatusCode::BAD_REQUEST
    );
    // A permission answer for a session without an agent is a conflict.
    let (status, _) = body(
        answer_permission(
            State(st.clone()),
            Json(json!({"sessionId": id, "id": "ask-1"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = body(
        answer_permission(
            State(st.clone()),
            Json(json!({"sessionId": "chat-404", "id": "ask-1"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    std::fs::remove_dir_all(&st.root).ok();
}

// A slash command runs in place of a turn: the input, the user's line, the reply,
// and the idle state land in order. A message id is idempotent: the same text
// answers the recorded input again and appends nothing, other text conflicts.
#[tokio::test]
async fn a_slash_command_answers_in_place_and_message_ids_are_idempotent() {
    let st = state("slash");
    configure(&st, &[("ds_main", "Main")]);
    let id = open(&st).await;
    let (status, accepted) = prompt(&st, &id, json!({"text":"/help","messageId":"m1"})).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    let (_, v) = get(&st, &id).await;
    assert_eq!(v["state"], "idle");
    assert_eq!(
        kinds(&v),
        ["input", "user_message", "agent_message", "state"],
        "{v}"
    );
    assert!(agent_text(&v).contains("Commands:"), "{v}");
    assert_eq!(v["inputs"][0]["messageId"], "m1");
    let n = v["updates"].as_array().unwrap().len();
    let (status, again) = prompt(&st, &id, json!({"text":"/help","messageId":"m1"})).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(again["messageId"], "m1");
    assert_eq!(
        get(&st, &id).await.1["updates"].as_array().unwrap().len(),
        n
    );
    let (status, _) = prompt(&st, &id, json!({"text":"/status","messageId":"m1"})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    // An input to an idle conversation is a prompt.
    let (status, _) = input(&st, &id, json!({"text":"/help","messageId":"m2"})).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(get(&st, &id).await.1["inputs"].as_array().unwrap().len(), 2);
    std::fs::remove_dir_all(&st.root).ok();
}

// A prompt that is not a slash command goes to the agent, even when its first word
// names a command; an agent that cannot start fails the turn with the reason.
#[tokio::test]
async fn a_prompt_reaches_the_agent_and_a_missing_agent_fails_the_turn() {
    let st = state("agent");
    configure(&st, &[("ds_main", "Main")]);
    for text in ["release the pending work", "status of the cart?", "help me"] {
        assert_eq!(run_command(&st, text).await, None, "{text}");
    }
    let id = open(&st).await;
    let (status, accepted) = prompt(
        &st,
        &id,
        json!({"text":"release the pending work","messageId":"m1"}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    assert_eq!(accepted["deliveryMode"], "steer");
    let v = settle(&st, &id, "failed").await;
    let said = agent_text(&v);
    assert!(said.contains("agent failed to start"), "{said}");
    assert!(said.contains(NO_AGENT), "{said}");
    assert!(!said.contains("released"), "{said}");
    assert!(!st.chat.is_running(&id));
    // A failed conversation takes the next prompt.
    let (status, _) = prompt(&st, &id, json!({"text":"/help","messageId":"m2"})).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(get(&st, &id).await.1["state"], "idle");
    std::fs::remove_dir_all(&st.root).ok();
}

// While a turn runs: a prompt is refused toward /inputs, a slash command waits, a
// stale turn id conflicts, and an input without a steering agent queues for the
// next turn (once per message id). Cancel stops the turn and cancels every queued
// input; a stopping turn takes no more input.
#[tokio::test]
async fn inputs_queue_while_a_turn_runs_and_cancel_clears_them() {
    let st = state("running");
    configure(&st, &[("ds_main", "Main")]);
    let id = open(&st).await;
    {
        let s = session(&st, &id);
        let mut s = s.lock().unwrap();
        s.state = "running".into();
        s.turn_id = Some(format!("{}:m0", id));
    }
    assert!(st.chat.is_running(&id));
    assert_eq!(
        prompt(&st, &id, json!({"text":"more","messageId":"p1"}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        input(&st, &id, json!({"text":"/status","messageId":"i0"}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        input(
            &st,
            &id,
            json!({"text":"more","messageId":"i0","expectedTurnId":"other"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, queued) = input(
        &st,
        &id,
        json!({"text":"also check totals","messageId":"i1","expectedTurnId":format!("{}:m0", id)}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{queued}");
    assert_eq!(queued["state"], "queued");
    assert_eq!(queued["deliveryMode"], "next_turn");
    assert_eq!(queued["admitting"], false);
    let (status, again) = input(
        &st,
        &id,
        json!({"text":"also check totals","messageId":"i1"}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(again["sequence"], queued["sequence"]);
    input(&st, &id, json!({"text":"and taxes","messageId":"i2"})).await;
    assert_eq!(session(&st, &id).lock().unwrap().inputs.len(), 2);

    let r = cancel(State(st.clone()), Path(id.clone())).await;
    assert_eq!(r.status(), StatusCode::ACCEPTED);
    assert!(!st.chat.is_running(&id));
    let (_, v) = get(&st, &id).await;
    let states: Vec<Value> = v["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["state"].clone())
        .collect();
    assert_eq!(states, vec![json!("cancelled"), json!("cancelled")]);
    assert_eq!(
        input(&st, &id, json!({"text":"late","messageId":"i3"}))
            .await
            .0,
        StatusCode::CONFLICT,
        "a stopping turn took input"
    );
    std::fs::remove_dir_all(&st.root).ok();
}

// Delete removes the conversation, announces the list, and removes its transcript,
// so a restart does not bring it back.
#[tokio::test]
async fn delete_removes_the_conversation_and_its_transcript() {
    let st = state("delete");
    configure(&st, &[("ds_main", "Main")]);
    let kept = open(&st).await;
    let gone = open(&st).await;
    prompt(&st, &gone, json!({"text":"/help"})).await;
    let mut events = st.events.subscribe();
    let (status, _) = body(delete_session(State(st.clone()), Path(gone.clone())).await).await;
    assert_eq!(status, StatusCode::OK);
    let announced = events.try_recv().unwrap();
    assert_eq!(announced["type"], "chat.sessions");
    assert_eq!(get(&st, &gone).await.0, StatusCode::NOT_FOUND);
    let restarted = ChatManager::default();
    restarted.restore(&st.out);
    let ids: Vec<Value> = restarted
        .snapshot()
        .iter()
        .map(|s| s["id"].clone())
        .collect();
    assert_eq!(ids, vec![json!(kept)]);
    assert!(crate::acp::sessions::read(&st.out, &gone).is_empty());
    std::fs::remove_dir_all(&st.root).ok();
}

// After a restart every stored conversation comes back with its updates and the
// state its last record implies, and numbering continues past the highest id.
#[tokio::test]
async fn restore_after_restart_keeps_states_updates_and_numbering() {
    let st = state("restore");
    configure(&st, &[("ds_main", "Main")]);
    let answered = open(&st).await;
    prompt(&st, &answered, json!({"text":"/help"})).await;
    let quiet = open(&st).await;
    let record = |id: &str, update: Value| {
        crate::acp::sessions::record_update_checked(&st.out, id, &update).unwrap();
    };
    crate::acp::sessions::open(&st.out, "chat-7", &st.root, "gui");
    record(
        "chat-7",
        json!({"sessionUpdate":"user_message","text":"go","messageId":"a"}),
    );
    record(
        "chat-7",
        json!({"sessionUpdate":"turn_end","stop":"cancelled"}),
    );
    crate::acp::sessions::open(&st.out, "chat-9", &st.root, "gui");
    record("chat-9", json!({"sessionUpdate":"state","state":"failed"}));
    crate::acp::sessions::open(&st.out, "chat-11", &st.root, "gui");
    record(
        "chat-11",
        json!({"sessionUpdate":"turn_end","stop":"end_turn"}),
    );
    record(
        "chat-11",
        json!({"sessionUpdate":"agent_message_chunk","content":{"text":"partial"}}),
    );
    let before = get(&st, &answered).await.1;

    let restarted = ChatManager::default();
    restarted.restore(&st.out);
    let snap: BTreeMap<String, Value> = restarted
        .snapshot()
        .into_iter()
        .map(|s| (s["id"].as_str().unwrap().to_string(), s))
        .collect();
    assert_eq!(snap.len(), 5, "{snap:?}");
    assert_eq!(snap[&answered]["state"], "idle");
    assert_eq!(
        snap[&answered]["updates"],
        json!(before["updates"].as_array().unwrap().len())
    );
    assert_eq!(snap[&quiet]["state"], "idle");
    assert_eq!(snap[&quiet]["updates"], 0);
    assert_eq!(snap["chat-7"]["state"], "cancelled");
    assert_eq!(snap["chat-9"]["state"], "failed");
    assert_eq!(snap["chat-11"]["state"], "idle");
    assert_eq!(restarted.seq.load(Ordering::Relaxed), 11);
    // A second restore of a manager that already holds sessions changes nothing.
    restarted.restore(&st.out);
    assert_eq!(restarted.snapshot().len(), 5);
    std::fs::remove_dir_all(&st.root).ok();
}

// Slash commands against the project's document sets: with none, a graph command
// says how to add one; with one, it addresses that set; with several, it asks which
// one (bare /compile queues the dirty ones, here none). Help works in all three.
#[tokio::test]
async fn slash_commands_with_zero_one_and_several_document_sets() {
    let st = state("sets");
    configure(&st, &[]);
    let help = run_command(&st, "/help").await.unwrap();
    assert!(help.contains("Commands:"), "{help}");
    for cmd in ["/status", "/board", "/questions", "/compile"] {
        let reply = run_command(&st, cmd).await.unwrap();
        assert!(reply.contains("no document set yet"), "{cmd}: {reply}");
    }

    configure(&st, &[("ds_main", "Main")]);
    let status = run_command(&st, "/status").await.unwrap();
    assert!(status.starts_with("generation 0"), "{status}");
    let questions = run_command(&st, "/questions").await.unwrap();
    assert!(
        questions.starts_with("no standing questions"),
        "{questions}"
    );
    let board = run_command(&st, "/board").await.unwrap();
    assert!(!board.contains("choose a document set"), "{board}");
    assert!(run_command(&st, "/model").await.is_some());
    assert!(run_command(&st, "/config").await.is_some());
    let agent = run_command(&st, "/agent").await.unwrap();
    assert!(!agent.is_empty());
    // Through the handler the reply is the turn's agent message.
    let id = open(&st).await;
    prompt(&st, &id, json!({"text":"/status"})).await;
    let v = get(&st, &id).await.1;
    assert!(agent_text(&v).starts_with("generation 0"), "{v}");
    assert!(!st.root.join(".jazyk/status.yaml").exists());

    configure(&st, &[("ds_orders", "Orders"), ("ds_billing", "Billing")]);
    for cmd in ["/status", "/board", "/questions", "/explain", "/ripple"] {
        let reply = run_command(&st, cmd).await.unwrap();
        assert!(reply.starts_with("choose a document set"), "{cmd}: {reply}");
        assert!(
            reply.contains("Orders") && reply.contains("Billing"),
            "{reply}"
        );
    }
    assert_eq!(
        run_command(&st, "/compile").await.unwrap(),
        "no dirty document sets"
    );
    let help = run_command(&st, "/help").await.unwrap();
    assert!(help.contains("/compile"), "{help}");
    std::fs::remove_dir_all(&st.root).ok();
}
