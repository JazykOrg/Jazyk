use super::*;

#[test]
fn checkpoint_restores_staged_batch_and_its_gate_state() {
    let dir = bare_dir("checkpoint-batch");
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
    let call = |server: &McpServer, name: &str, arguments: Value| {
        let result = server
            .handle("tools/call", &json!({"name":name,"arguments":arguments}))
            .unwrap();
        assert_ne!(result["isError"], true, "{result}");
        serde_json::from_str::<Value>(result["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let original = make();
    call(&original, "begin_goals", json!({}));
    call(
        &original,
        "set_coverage",
        json!({"section":"docs/a.md#/a", "state":"non-normative", "note":"Navigation."}),
    );
    let goals = original
        .open
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .goals
        .iter()
        .map(|g| g.id.clone())
        .collect::<Vec<_>>();
    let checkpoint = original.checkpoint().unwrap();
    let generation = crate::store::read_generation(&out);
    assert!(!checkpoint["open"]["session"]["staged"]
        .as_array()
        .unwrap()
        .is_empty());
    drop(original);
    let restored = make();
    restored.restore(checkpoint.clone()).unwrap();
    assert_eq!(restored.checkpoint().unwrap(), checkpoint);
    let claims = goals.iter().map(|goal| json!({"goal":goal,"justification":"The section is a navigation index and is marked non-normative."})).collect::<Vec<_>>();
    let result = call(
        &restored,
        "done",
        json!({"summary":"Recorded the navigation coverage.","goals":claims}),
    );
    assert_eq!(result["committed"], true, "{result}");
    assert!(crate::store::read_generation(&out) > generation);
    drop(restored);
    crate::vfs::remove_dir_all(dir).unwrap();
}

// A scoped serving whose batch committed answers every later write, goal tool,
// done, and abandon with session-complete, naming the batch; an unscoped serving
// keeps the no-open-batch refusal. Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
#[test]
fn a_committed_scoped_serving_says_the_session_is_complete() {
    let dir = bare_dir("session-complete");
    let project = crate::document_sets::Workspace::load(&dir)
        .unwrap()
        .resolve(Some("ds_main"))
        .unwrap();
    let out = project.out.clone();
    let flags = BridgeFlags {
        only: Some("b7-1".into()),
        ..BridgeFlags::default()
    };
    let s = McpServer::with_bridge(
        project.clone(),
        out.clone(),
        vec!["compile".to_string()],
        true,
        flags,
    );
    *s.finished.lock().unwrap() = Some("b7-1".into());
    let call = |name: &str, args: Value| {
        let r = s
            .handle("tools/call", &json!({"name": name, "arguments": args}))
            .unwrap();
        let text = r["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        (r["isError"].as_bool().unwrap_or(false), text)
    };
    for (name, args) in [
        (
            "mark_goal_done",
            json!({"goal": "g:reconcile-section:docs/a.md#/a", "justification": "Done."}),
        ),
        ("upsert_entity", json!({"name": "Order"})),
        ("done", json!({"summary": "again"})),
        ("abandon_goals", json!({})),
    ] {
        let (is_err, text) = call(name, args);
        assert!(is_err, "{}: {}", name, text);
        assert!(
            text.contains("session-complete")
                && text.contains("b7-1")
                && text.contains("end your turn"),
            "{}: {}",
            name,
            text
        );
    }
    let open = McpServer::with_bridge(
        project,
        out,
        vec!["compile".to_string()],
        true,
        BridgeFlags::default(),
    );
    *open.finished.lock().unwrap() = Some("b7-1".into());
    let r = open
        .handle(
            "tools/call",
            &json!({"name": "done", "arguments": {"summary": "x"}}),
        )
        .unwrap();
    assert!(r["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("no-open-batch"));
}

// The agent-run benchmark runs a derived-goal case end to end: the fixture seeds
// and derives, the session opens holding its prompt's loaded set, and finish_case
// commits the staged work before grading. A serving that committed only extraction
// and review cases scored an agent zero on three correct criteria.
// Mirrors docs/benchmark/benchmark.md#agent-run-benchmarks.
#[test]
fn benchmark_serving_commits_a_derived_goal_case() {
    let dir = bare_dir("bench-derived");
    let project = crate::project::Project::load(&dir);
    let out = project.out.clone();
    let s = McpServer::with_bridge(
        project,
        out,
        vec!["benchmark".to_string()],
        false,
        BridgeFlags::default(),
    );
    let call = |name: &str, args: Value| -> Value {
        let r = s
            .handle("tools/call", &json!({"name": name, "arguments": args}))
            .unwrap();
        let text = r["content"][0]["text"].as_str().unwrap_or_default();
        serde_json::from_str(text).unwrap_or(Value::String(text.to_string()))
    };
    let begun = call("begin_case", json!({"case": "criteria-density"}));
    assert_eq!(begun["case"]["name"], "criteria-density", "{}", begun);
    assert!(
        begun["instructions"]
            .as_str()
            .unwrap_or_default()
            .contains("derive-criteria"),
        "the prompt names the goal: {}",
        begun["instructions"]
    );
    assert!(
        begun["package"]
            .as_str()
            .unwrap_or_default()
            .contains("req:company-1"),
        "the session opens holding its prompt's loaded set: {}",
        begun["package"]
    );
    let staged = call(
        "upsert_criterion",
        json!({
            "requirement": "req:company-1",
            "kind": "positive",
            "statement": "An observer counting the company's departments finds exactly 4 departments.",
            "provenance": {"derived": {"from": ["req:company-1"], "reasoning": "covers the stated count"}},
        }),
    );
    assert!(staged["id"].is_string(), "{}", staged);
    let second = call(
        "upsert_criterion",
        json!({
            "requirement": "req:company-2",
            "kind": "boundary",
            "statement": "A claim for `500` dollars needs no approval from the head of department.",
            "provenance": {"derived": {"from": ["req:company-2"], "reasoning": "the value at the edge"}},
        }),
    );
    assert!(second["id"].is_string(), "{}", second);
    let third = call(
        "upsert_criterion",
        json!({
            "requirement": "req:company-2",
            "kind": "boundary",
            "statement": "A claim for `501` dollars needs the head of department's approval.",
            "provenance": {"derived": {"from": ["req:company-2"], "reasoning": "the value past the edge"}},
        }),
    );
    assert!(third["id"].is_string(), "{}", third);
    let marked = call(
        "mark_goal_done",
        json!({
            "goal": "g:derive-criteria:ent:company",
            "justification": "Each requirement carries the criteria its shape needs.",
            "evidence": {"requirements": [
                {"id": "req:company-1", "criteria": [staged["id"]], "verdict": "complete"},
                {"id": "req:company-2", "criteria": [second["id"], third["id"]], "verdict": "complete"},
            ]},
        }),
    );
    assert!(marked["marked"].is_string(), "{}", marked);
    let scored = call("finish_case", json!({"summary": "criteria derived"}));
    assert_eq!(scored["scored"]["score"], json!(1.0), "{}", scored);
    // One line per case, so a long list stays readable to an agent whose client
    // truncates replies (Codex's guard refused a case it could not see listed).
    let list = call("benchmark_cases", json!({}));
    let rows = list["cases"].as_array().unwrap();
    assert!(rows.iter().all(|r| r.is_string()), "{}", list);
    assert!(
        rows.iter()
            .any(|r| r == "criteria-density (review, derive-criteria): scored 1.0"),
        "{}",
        list
    );
    // Feedback answers with no graph in the serving's out directory.
    let fed = call(
        "report_feedback",
        json!({"kind": "wrong", "subject": "begin_case", "message": "a test note"}),
    );
    assert!(fed.get("error").is_none(), "{}", fed);
}

// The compile serving claims goal batches: begin_goals claims the next ready
// batch under a per-batch lease naming its goals, done without a mark per
// mandatory goal refuses with open-goal, and abandon_goals releases the claim.
// Mirrors docs/frontends/mcp.md#compilation-over-mcp.
#[test]
fn begin_goals_claims_the_next_batch_and_done_wants_marks() {
    let dir = std::env::temp_dir().join(format!("jazyk-mcp-goals-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&dir).ok();
    crate::vfs::create_dir_all(dir.join("docs")).unwrap();
    crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
    crate::vfs::write(dir.join("docs/a.md"), "# A\n\nThe body.\n").unwrap();
    let project = crate::project::Project::load(&dir);
    let out = project.out.clone();
    crate::vfs::create_dir_all(&out).unwrap();
    let control = crate::control::Control {
        compile: "auto".into(),
        ..Default::default()
    };
    control.save(&out);
    let server = McpServer::new(project, out.clone(), vec!["compile".into()], false);

    let v = server.begin_goals(&json!({"arguments": {}}));
    assert!(v["error"].is_null(), "{}", v);
    // The batch id is b<generation>-<n>: the sync that absorbed the fresh
    // document bumped the generation before the board derived.
    let batch = v["batch"].as_str().unwrap().to_string();
    assert!(batch.starts_with('b') && batch.contains('-'), "{}", batch);
    let instructions = v["instructions"].as_str().unwrap();
    assert!(instructions.contains("## Goals"), "{}", instructions);
    assert!(
        instructions.contains(&batch),
        "the protocol line names the batch"
    );
    assert!(v["package"].as_str().unwrap().contains("## Loaded"));
    // The open batch is recorded, naming its goals, for information.
    let leases = crate::control::leases(&out);
    let lease = leases
        .iter()
        .find(|l| l.task == batch)
        .expect("an open batch entry");
    assert!(
        !lease.goals.is_empty() && lease.goals.iter().all(|g| g.starts_with("g:")),
        "{:?}",
        lease.goals
    );
    // A second begin refuses while the batch is open.
    let again = server.begin_goals(&json!({"arguments": {}}));
    assert_eq!(again["error"]["rule"], "batch-open");
    // done without a mark per mandatory goal refuses with open-goal and keeps
    // the changeset open.
    let d = server.done_batch(&json!({"arguments": {"summary": "did nothing"}}));
    assert_eq!(d["error"]["rule"], "open-goal", "{}", d);
    let a = server.abandon_goals(&json!({"arguments": {"reason": "test"}}));
    assert_eq!(a["abandoned"], json!(batch));
    assert!(crate::control::leases(&out).is_empty());
    crate::vfs::remove_dir_all(&dir).ok();
}

// report_spend is served with every toolset, the read-only graph serving included,
// and outside an open batch it is refused toward `jazyk spend`.
// Mirrors docs/compiler/tools.md#spend-tool.
#[test]
fn report_spend_is_served_everywhere_and_refused_outside_a_batch() {
    let dir = bare_dir("spend-tool");
    let project = crate::project::Project::load(&dir);
    let out = project.out.clone();
    let graph = McpServer::new(
        crate::project::Project::load(&dir),
        out.clone(),
        vec!["graph".into()],
        false,
    );
    assert!(tool_names(&graph).iter().any(|n| n == "report_spend"));
    let r = graph
        .handle(
            "tools/call",
            &json!({"name": "report_spend", "arguments": {"tokens": 500}}),
        )
        .unwrap();
    assert_eq!(r["isError"], true);
    let text = r["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("no-open-batch") && text.contains("jazyk spend"),
        "{}",
        text
    );
    let compile = McpServer::new(project, out, vec!["compile".into()], false);
    assert!(tool_names(&compile).iter().any(|n| n == "report_spend"));
    let r = compile
        .handle(
            "tools/call",
            &json!({"name": "report_spend", "arguments": {"tokens": 500}}),
        )
        .unwrap();
    assert_eq!(r["isError"], true);
    assert!(r["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("no-open-batch"));
    crate::vfs::remove_dir_all(&dir).ok();
}
#[test]
fn stop_fence_discards_implicit_finish_and_rejects_late_done() {
    let dir = bare_dir("stop-fence");
    crate::vfs::write(dir.join("docs/a.md"), "# A\n\nNavigation index.\n").unwrap();
    let project = crate::document_sets::Workspace::load(&dir)
        .unwrap()
        .resolve(Some("ds_main"))
        .unwrap();
    let out = project.out.clone();
    let mut control = crate::control::Control::load(&project, &out);
    control.compile = "auto".into();
    control.save(&out);
    let fence = out.join("cancel-test");
    let server = McpServer::with_bridge(
        project,
        out.clone(),
        vec!["compile".into()],
        true,
        BridgeFlags {
            ephemeral: true,
            cancel_file: Some(fence.clone()),
            ..Default::default()
        },
    );
    let call = |name: &str, arguments: Value| {
        let v = server
            .handle("tools/call", &json!({"name":name, "arguments":arguments}))
            .unwrap();
        assert_ne!(v["isError"], true, "{v}");
        serde_json::from_str::<Value>(v["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    call("begin_goals", json!({}));
    call(
        "set_coverage",
        json!({"section":"docs/a.md#/a", "state":"non-normative", "note":"Navigation."}),
    );
    let generation = crate::store::read_generation(&out);
    assert!(!server
        .open
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .session
        .staged
        .is_empty());
    crate::vfs::write(&fence, "stopped").unwrap();
    assert!(server
        .handle("tools/call", &json!({"name":"done", "arguments":{}}))
        .is_err());
    server.finish();
    assert!(server.open.lock().unwrap().is_none());
    assert_eq!(
        crate::store::read_generation(&out),
        generation,
        "EOF committed cancelled work"
    );
    crate::vfs::remove_dir_all(dir).unwrap();
}

// A serving with no open batch builds a session per call, but the repeated-call
// guard and the feedback cap carry across its calls.
// Mirrors docs/compiler/sessions.md#repeated-calls and docs/compiler/tools.md#feedback-tool.
#[test]
fn per_call_sessions_keep_the_guard_and_the_feedback_cap() {
    let dir = bare_dir("call-memory");
    crate::vfs::write(dir.join("docs/a.md"), "# A\n\nThe tool sorts lines.\n").unwrap();
    let project = crate::document_sets::Workspace::load(&dir)
        .unwrap()
        .resolve(Some("ds_main"))
        .unwrap();
    let (parsed, _) = crate::reconcile::parse_all(&project);
    let mut store = Store::load(&project.out);
    store.sync_docs(&parsed);
    store.save_for_test();
    let serving = McpServer::new(
        project.clone(),
        project.out.clone(),
        vec!["graph".into()],
        false,
    );
    let call = |name: &str, args: Value| {
        serving
            .handle("tools/call", &json!({"name":name,"arguments":args}))
            .unwrap()
    };
    let first = call("search", json!({"query":"sorts"}));
    assert_ne!(first["isError"], true, "{first}");
    call("search", json!({"query":"sorts"}));
    let third = call("search", json!({"query":"sorts"}));
    assert_eq!(third["isError"], true, "{third}");
    assert!(
        third["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("repeated-call"),
        "{third}"
    );
    for n in 0..7 {
        call(
            "report_feedback",
            json!({"kind":"other","message":format!("note {n}")}),
        );
    }
    let log = crate::vfs::read_to_string(crate::feedback::path(&project.out)).unwrap_or_default();
    assert_eq!(log.lines().count(), crate::tools::FEEDBACK_LIMIT, "{log}");
    crate::vfs::remove_dir_all(dir).ok();
}

// Nothing claims work: two servings open the same batch, both finish, the first
// commit wins and the second gets a commit conflict with the batch still open on the
// current generation. Mirrors docs/compiler/graph.md#commit-conflicts.
#[test]
fn two_servings_overlap_warn_and_both_commit_with_recorded_overwrite() {
    let dir = bare_dir("race-same");
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
    let text = |result: Value| {
        serde_json::from_str::<Value>(result["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let call = |server: &McpServer, name: &str, arguments: Value| {
        text(
            server
                .handle("tools/call", &json!({"name":name,"arguments":arguments}))
                .unwrap(),
        )
    };
    let (one, two) = (make(), make());
    let a = call(&one, "begin_goals", json!({}));
    let b = call(&two, "begin_goals", json!({}));
    assert!(a["error"].is_null() && b["error"].is_null(), "{a} {b}");
    // The same goals under two batch ids: a batch id skips the ids already open.
    assert_eq!(a["goals"], b["goals"], "both opened the same work");
    // Both are visible as open, and neither refused the other.
    let goal = a["goals"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(
        crate::control::leases(&out)
            .iter()
            .filter(|l| l.goals.contains(&goal))
            .count(),
        2
    );
    let staged_one = call(
        &one,
        "set_coverage",
        json!({"section":"docs/a.md#/a","state":"non-normative","note":"First navigation review."}),
    );
    assert!(staged_one["error"].is_null(), "{staged_one}");
    let staged_two = call(
        &two,
        "set_coverage",
        json!({"section":"docs/a.md#/a","state":"non-normative","note":"Second navigation review."}),
    );
    assert_eq!(
        staged_two["warnings"].as_array().unwrap().len(),
        1,
        "{staged_two}"
    );
    assert!(staged_two["warnings"][0]["message"]
        .as_str()
        .unwrap()
        .contains("commits first"));
    let read = call(&one, "graph_status", json!({}));
    assert_eq!(read["warnings"].as_array().unwrap().len(), 1, "{read}");
    let finish = |server: &McpServer| {
        let goals = server.open.lock().unwrap().as_ref().unwrap().goals.clone();
        let claims = goals
            .iter()
            .map(|g| json!({"goal":g.id,"justification":"The section is a navigation index and is marked non-normative."}))
            .collect::<Vec<_>>();
        call(
            server,
            "done",
            json!({"summary":"Recorded coverage.","goals":claims}),
        )
    };
    let first = finish(&one);
    assert_eq!(first["committed"], true, "{first}");
    let noticed = call(&two, "graph_status", json!({}));
    assert!(
        !noticed["conflictNotices"].as_array().unwrap().is_empty(),
        "{noticed}"
    );
    let second = finish(&two);
    assert_eq!(second["committed"], true, "{second}");
    assert!(
        !second["overwrites"].as_array().unwrap().is_empty(),
        "{second}"
    );
    assert!(
        !second["conflictNotices"].as_array().unwrap().is_empty(),
        "{second}"
    );
    let generation = second["generation"].as_u64().unwrap();
    let entry = crate::goals::read_journal_entry(
        &crate::layout::journal_file(&out, generation),
        generation,
    )
    .unwrap();
    assert!(!entry.overwrites.is_empty());
    assert!(entry
        .overwrites
        .iter()
        .any(|c| c["priorValue"] == "First navigation review."
            && c["value"] == "Second navigation review."));
    assert!(Store::load(&out)
        .graph
        .diagnostics
        .values()
        .any(|d| d.rule == "concurrent-overwrite" && d.severity == "info"));
    crate::vfs::remove_dir_all(dir).unwrap();
}

// Disjoint work rebases: a changeset staged before another commit landed commits on
// top of it, and its journal entry records `rebased_from`.
#[test]
fn a_disjoint_changeset_rebases_onto_a_later_commit() {
    let dir = bare_dir("race-disjoint");
    // Both documents are roots, so both reconcile at level 0 and their batches are
    // ready at once.
    crate::vfs::write(
        dir.join(".jazyk/jazyk.toml"),
        "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = [\"docs/a.md\", \"docs/b.md\"]\nsource = []\n",
    )
    .unwrap();
    crate::vfs::write(dir.join("docs/a.md"), "# A\n\nNavigation index.\n").unwrap();
    crate::vfs::write(dir.join("docs/b.md"), "# B\n\nAnother index.\n").unwrap();
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
    let call = |server: &McpServer, name: &str, arguments: Value| {
        let r = server
            .handle("tools/call", &json!({"name":name,"arguments":arguments}))
            .unwrap();
        serde_json::from_str::<Value>(r["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let goals_for = |doc: &str| -> Vec<String> {
        crate::board::Board::compute(&project, &out)
            .open_goals()
            .iter()
            .filter(|g| g.target.starts_with(doc))
            .map(|g| g.id.clone())
            .collect()
    };
    let (one, two) = (make(), make());
    let a_goals = goals_for("docs/a.md");
    let b_goals = goals_for("docs/b.md");
    assert!(!a_goals.is_empty() && !b_goals.is_empty());
    let a = call(&one, "begin_goals", json!({"goals": a_goals}));
    let b = call(&two, "begin_goals", json!({"goals": b_goals}));
    assert!(a["error"].is_null() && b["error"].is_null(), "{a} {b}");
    let finish = |server: &McpServer, doc: &str, section: &str| {
        call(
            server,
            "set_coverage",
            json!({"section":format!("{doc}#{section}"), "state":"non-normative", "note":"Navigation."}),
        );
        let goals = server.open.lock().unwrap().as_ref().unwrap().goals.clone();
        let claims = goals
            .iter()
            .map(|g| json!({"goal":g.id,"justification":"The section is a navigation index and is marked non-normative."}))
            .collect::<Vec<_>>();
        call(
            server,
            "done",
            json!({"summary":"Recorded coverage.","goals":claims}),
        )
    };
    let first = finish(&one, "docs/a.md", "/a");
    assert_eq!(first["committed"], true, "{first}");
    let second = finish(&two, "docs/b.md", "/b");
    assert_eq!(second["committed"], true, "{second}");
    let since = second["rebasedFrom"]
        .as_u64()
        .expect("the second commit rebased");
    let generation = second["generation"].as_u64().unwrap();
    let entry: crate::model::JournalEntry = serde_norway::from_str(
        &crate::vfs::read_to_string(crate::layout::journal_file(&out, generation)).unwrap(),
    )
    .unwrap();
    assert_eq!(entry.rebased_from, Some(since));
    crate::vfs::remove_dir_all(dir).unwrap();
}

// One serving holds several open batches: a second begin opens another, a call that
// stages into a batch must name it once several are open, and naming it works on it.
// Mirrors docs/frontends/mcp.md#the-control-plane-over-mcp.
#[test]
fn one_serving_holds_several_open_batches_and_calls_name_theirs() {
    let dir = bare_dir("several-open");
    crate::vfs::write(dir.join("docs/a.md"), "# A\n\nNavigation index.\n").unwrap();
    let project = crate::document_sets::Workspace::load(&dir)
        .unwrap()
        .resolve(Some("ds_main"))
        .unwrap();
    let out = project.out.clone();
    let mut control = crate::control::Control::load(&project, &out);
    control.compile = "auto".into();
    control.save(&out);
    let server = McpServer::new(project, out.clone(), vec!["compile".into()], false);
    let call = |name: &str, arguments: Value| {
        let r = server
            .handle("tools/call", &json!({"name":name,"arguments":arguments}))
            .unwrap();
        serde_json::from_str::<Value>(r["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let first = call("begin_goals", json!({}));
    let second = call("begin_goals", json!({}));
    assert!(
        first["error"].is_null() && second["error"].is_null(),
        "{first} {second}"
    );
    let (one, two) = (first["batch"].clone(), second["batch"].clone());
    assert_ne!(one, two);
    let refused = call("abandon_goals", json!({"reason":"which one?"}));
    assert_eq!(refused["error"]["rule"], "batch-required", "{refused}");
    assert_eq!(refused["error"]["open"].as_array().unwrap().len(), 2);
    let a = call("abandon_goals", json!({"reason":"first","batch":one}));
    assert_eq!(a["abandoned"], one, "{a}");
    // With one left, calls act on it without naming it.
    let b = call("abandon_goals", json!({"reason":"second"}));
    assert_eq!(b["abandoned"], two, "{b}");
    // The tool list offers `batch` on the calls that act on a batch.
    let list = server.handle("tools/list", &json!({})).unwrap();
    let done = list["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "done")
        .unwrap();
    assert!(done["inputSchema"]["properties"]["batch"].is_object());
    crate::vfs::remove_dir_all(&dir).ok();
}

#[test]
fn durable_open_work_survives_disconnect_and_expired_adoption_fences_old_owner() {
    let dir = bare_dir("durable-open-adopt");
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
    let call = |server: &McpServer, name: &str, args: Value| {
        let r = server
            .handle("tools/call", &json!({"name":name,"arguments":args}))
            .unwrap();
        serde_json::from_str::<Value>(r["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let one = make();
    let opened = call(&one, "begin_goals", json!({}));
    let id = opened["workId"].as_str().unwrap();
    let token = opened["ownerToken"].clone();
    let staged = call(
        &one,
        "set_coverage",
        json!({"section":"docs/a.md#/a","state":"non-normative","note":"Retained navigation review."}),
    );
    assert!(staged["error"].is_null(), "{staged}");
    let generation = crate::store::read_generation(&out);
    one.eof_finish();
    drop(one);
    assert_eq!(
        crate::store::read_generation(&out),
        generation,
        "disconnect must not commit"
    );
    let two = make();
    let resumed = call(&two, "begin_goals", json!({"workId":id,"ownerToken":token}));
    assert_eq!(resumed["resumed"], true, "{resumed}");
    assert_eq!(
        two.open
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .session
            .staged
            .len(),
        1
    );
    let mut r = crate::open_work::read(&out, id).unwrap();
    let seq = r.sequence;
    r.expires_at = 0;
    crate::open_work::save(&out, &mut r, Some(seq)).unwrap();
    let three = make();
    let adopted = call(
        &three,
        "adopt_open_work",
        json!({"id":id,"expectedEpoch":r.receipt.epoch,"reason":"Recover interrupted work."}),
    );
    assert_eq!(adopted["resumed"], true, "{adopted}");
    assert_ne!(adopted["ownerToken"], token);
    let stale = call(
        &two,
        "set_coverage",
        json!({"section":"docs/a.md#/a","state":"non-normative","note":"Late old owner."}),
    );
    assert_eq!(stale["error"]["rule"], "open-work-owner", "{stale}");
    let goals = three.open.lock().unwrap().as_ref().unwrap().goals.clone();
    let claims = goals
        .iter()
        .map(|g| json!({"goal":g.id,"justification":"Navigation index marked non-normative."}))
        .collect::<Vec<_>>();
    let done = call(
        &three,
        "done",
        json!({"summary":"Recovered navigation coverage.","goals":claims}),
    );
    assert_eq!(done["committed"], true, "{done}");
    let audit = crate::vfs::read_to_string(out.join("local/journal/open-work.jsonl")).unwrap();
    assert!(audit.contains("previousOwner"), "{audit}");
    crate::vfs::remove_dir_all(dir).unwrap();
}

#[test]
fn crash_mid_session_leaves_a_resumable_changeset_on_disk() {
    const CHILD: &str = "JAZYK_OPEN_WORK_CRASH_FIXTURE";
    let child = std::env::var_os(CHILD).map(std::path::PathBuf::from);
    let dir = child.clone().unwrap_or_else(|| bare_dir("crash-resume"));
    if child.is_none() {
        crate::vfs::write(dir.join("docs/a.md"), "# A\n\nNavigation index.\n").unwrap();
    }
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
    let call = |s: &McpServer, name: &str, args: Value| {
        let r = s
            .handle("tools/call", &json!({"name":name,"arguments":args}))
            .unwrap();
        assert_ne!(r["isError"], true, "{r}");
        serde_json::from_str::<Value>(r["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    if child.is_some() {
        let s = make();
        call(&s, "begin_goals", json!({}));
        call(
            &s,
            "set_coverage",
            json!({"section":"docs/a.md#/a","state":"non-normative","note":"Crash-safe review."}),
        );
        crate::vfs::write(dir.join("ready"), "ready").unwrap();
        loop {
            std::thread::park();
        }
    }
    let mut process = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "mcp::tests::lifecycle::crash_mid_session_leaves_a_resumable_changeset_on_disk",
        ])
        .env(CHILD, &dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let started = std::time::Instant::now();
    while !dir.join("ready").exists() {
        if started.elapsed().as_secs() > 10 {
            process.kill().ok();
            panic!("crash fixture did not stage in time");
        }
        assert!(
            process.try_wait().unwrap().is_none(),
            "child exited before staging"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    process.kill().unwrap();
    process.wait().unwrap();
    let records = crate::open_work::list(&out).unwrap();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.mutations.len(), 1);
    let server = make();
    let resumed = call(
        &server,
        "begin_goals",
        json!({"workId":record.receipt.id,"ownerToken":record.receipt.owner}),
    );
    assert_eq!(resumed["resumed"], true);
    assert_eq!(
        server
            .open
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .session
            .staged
            .len(),
        1
    );
    let staged = call(&server, "read_open_work", json!({"id":record.receipt.id}));
    let mutation_id = staged["mutations"][0]["mutationId"].as_u64().unwrap();
    let unstaged = call(&server, "unstage", json!({"mutationIds":[mutation_id]}));
    assert_eq!(unstaged["stillStaged"], 0, "{unstaged}");
    call(
        &server,
        "set_coverage",
        json!({"section":"docs/a.md#/a","state":"non-normative","note":"Crash-safe review."}),
    );
    let restaged = call(&server, "read_open_work", json!({"id":record.receipt.id}));
    assert!(
        restaged["mutations"][0]["mutationId"].as_u64().unwrap() > mutation_id,
        "mutation IDs must not be reused"
    );
    let discarded = call(
        &server,
        "abandon_goals",
        json!({"reason":"Crash recovery verified."}),
    );
    assert!(discarded["error"].is_null());
    let closed = crate::open_work::read(&out, &record.receipt.id).unwrap();
    assert!(closed.mutations.is_empty());
    assert!(closed.payload.is_null());
    let log = crate::vfs::read_to_string(
        out.join("local/open")
            .join(&record.receipt.id)
            .join("events.jsonl"),
    )
    .unwrap();
    assert!(
        !log.contains("Crash-safe review."),
        "discard retained payload"
    );
    crate::vfs::remove_dir_all(dir).unwrap();
}
