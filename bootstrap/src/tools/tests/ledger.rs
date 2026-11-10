use super::*;

// report_spend records an external agent's own bill on the open batch: the commit
// takes it when the harness measured nothing, a measurement wins when it did, a
// later call replaces an earlier one, and outside a batch the call is refused
// toward `jazyk spend`. Mirrors docs/compiler/tools.md#spend-tool.
#[test]
fn reported_spend_rides_the_commit_when_nothing_was_measured() {
    let mut t = session();
    let ok = t
        .dispatch("report_spend", &json!({"tokens": 18400, "cached": 9100}))
        .unwrap();
    assert_eq!(ok["recorded"], json!(true));
    assert!(t.staged.is_empty(), "the report stages no mutation");
    let commit = t.commit(3, None);
    assert_eq!(commit.tokens, Some(18400));
    assert_eq!(commit.cached, Some(9100));
    assert_eq!(commit.tokens_source.as_deref(), Some("agent"));
    // The harness's own measurement wins over the report.
    let commit = t.commit(3, Some(500));
    assert_eq!(
        (commit.tokens, commit.cached, commit.tokens_source),
        (Some(500), None, None)
    );
    // A second call replaces the first and says so; a zero cached leaves the field out.
    let again = t
        .dispatch("report_spend", &json!({"tokens": 20000}))
        .unwrap();
    assert!(
        again["note"].as_str().unwrap_or("").contains("replaced"),
        "{}",
        again
    );
    let commit = t.commit(3, None);
    assert_eq!((commit.tokens, commit.cached), (Some(20000), None));
    // Bad numbers are refused and leave the report standing.
    assert_eq!(
        t.dispatch("report_spend", &json!({"tokens": 0}))
            .unwrap_err()
            .rule,
        "invalid-argument"
    );
    assert_eq!(
        t.dispatch("report_spend", &json!({"tokens": 10, "cached": 11}))
            .unwrap_err()
            .rule,
        "invalid-argument"
    );
    assert_eq!(t.commit(3, None).tokens, Some(20000));
    // Outside an open batch the call is refused toward the CLI form.
    let mut serving = ToolSession::new(Store::default(), WorkScope::serving("mcp-read"), 64, 24000);
    let err = serving
        .dispatch("report_spend", &json!({"tokens": 10}))
        .unwrap_err();
    assert_eq!(err.rule, "no-open-batch");
    assert!(err.message.contains("jazyk spend"), "{}", err.message);
    assert_eq!(serving.commit(1, None).tokens, None);
}

#[test]
fn feedback_logs_with_its_caller_and_stages_nothing() {
    let dir = std::env::temp_dir().join(format!("jazyk-tool-fb-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&dir).ok();
    let mut t = session();
    t.snapshot.out = dir.clone();
    t.caller = crate::feedback::Caller {
        source: "turn".into(),
        target: "shop.md".into(),
        model: "test-model".into(),
        codec: "native".into(),
        run: Some("run-1".into()),
        ..Default::default()
    };
    let ok = t
        .dispatch("report_feedback", &json!({"kind": "ambiguous", "subject": "set_coverage", "message": "covered vs non-normative is unclear for a section of links"}))
        .unwrap();
    assert_eq!(ok["recorded"], json!(true));
    assert!(t.staged.is_empty(), "feedback stages no mutation");

    let logged = crate::feedback::read(&dir, 10);
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0]["kind"], "ambiguous");
    assert_eq!(logged[0]["task"], "reconcile-section");
    assert_eq!(logged[0]["target"], "shop.md");
    assert_eq!(logged[0]["model"], "test-model");
    assert_eq!(logged[0]["run"], "run-1");

    // An empty message is the one rejection; an unknown kind is not.
    let err = t
        .dispatch(
            "report_feedback",
            &json!({"kind": "wrong", "message": "  "}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "missing-argument");
    t.dispatch(
        "report_feedback",
        &json!({"kind": "puzzled", "message": "second"}),
    )
    .unwrap();
    assert_eq!(crate::feedback::read(&dir, 10)[0]["kind"], "other");

    // The cap holds: further calls are acknowledged without a record.
    for i in 0..5 {
        t.dispatch(
            "report_feedback",
            &json!({"kind": "confusing", "message": format!("more {}", i)}),
        )
        .unwrap();
    }
    assert_eq!(crate::feedback::read(&dir, 99).len(), FEEDBACK_LIMIT);
    crate::vfs::remove_dir_all(&dir).ok();
}

#[test]
fn generation_workers_can_resolve_an_undecided_medium() {
    for task in [
        "generate",
        "bind",
        "generate-entity",
        "bind-requirement",
        "mcp-generate",
    ] {
        for name in GEN_TOOLS {
            assert!(toolset(task).contains(&name), "{task} lacks {name}");
        }
    }
    for kind in ["generate", "verify"] {
        let prompt = crate::goals::kind(kind).unwrap().prompt();
        assert!(
            !prompt.contains("run_command"),
            "{kind} advertises an unavailable command tool"
        );
        assert!(!toolset(kind).contains(&"run_command"));
        assert!(
            !catalog().iter().any(|t| t.name == "run_command"),
            "the catalog keeps an unserved command tool"
        );
        for name in crate::task_runtime::TOOLS {
            assert!(toolset(kind).contains(&name), "{kind} lacks {name}");
        }
    }
}

// Mirrors docs/consumers/gen.md#session-tasks-are-swept-when-unused.
#[test]
fn ledger_sessions_own_the_tasks_they_add_and_other_scopes_do_not() {
    let task = |id: &str| json!({"id":id,"name":id,"description":"d","command":"true"});
    for (kind, owned) in [("bind", true), ("reconcile-section", false)] {
        let files = std::collections::BTreeMap::from([(
            ".jazyk/jazyk.toml".into(),
            b"version=1\nsets=[]\n".to_vec(),
        )]);
        let _scope = crate::vfs::Scope::enter(&files);
        let mut tool = session();
        tool.gen.deliverable = "/project".into();
        tool.snapshot.out = "/project/.jazyk/ds_main".into();
        let goal = format!("g:{kind}:req:a-1");
        tool.scope.goals = vec![GoalScope {
            goal: goal.clone(),
            kind: kind.into(),
            ..Default::default()
        }];
        let revision = tool.dispatch("task_context", &json!({})).unwrap()["revision"].clone();
        tool.dispatch(
            "update_tasks",
            &json!({"expected_revision":revision,"tasks":[task("suite")]}),
        )
        .unwrap();
        let ledger = crate::gen::Ledger::load(std::path::Path::new("/project/.jazyk/ds_main"));
        assert_eq!(
            ledger.tasks.get("suite").map(|o| o.created_by.clone()),
            owned.then_some(goal),
            "{kind}"
        );
    }
}

#[test]
fn rejected_ledger_records_can_retry_after_test_source_repair() {
    for name in ["record_generation", "record_binding"] {
        let files = BTreeMap::from([
            ("hello.sh".into(), b"echo hello\n".to_vec()),
            (
                "tests/check.sh".into(),
                b"# wrong_selector\nexit 0\n".to_vec(),
            ),
        ]);
        let _scope = crate::vfs::Scope::enter(&files);
        let mut snapshot = Store {
            out: "/project/.jazyk/ds_main".into(),
            ..Default::default()
        };
        snapshot
            .graph
            .entities
            .insert("ent:greeter".into(), plain("Greeter"));
        snapshot.graph.requirements.insert(
            "req:hello".into(),
            Requirement {
                statement: "The Greeter prints hello.".into(),
                entities: vec!["ent:greeter".into()],
                ..Default::default()
            },
        );
        let goals = [
            Goal {
                id: "g:generate:ent:greeter".into(),
                kind: "generate".into(),
                target: "ent:greeter".into(),
                mandatory: true,
                ..Default::default()
            },
            Goal {
                id: "g:bind:req:hello".into(),
                kind: "bind".into(),
                target: "req:hello".into(),
                mandatory: true,
                ..Default::default()
            },
        ];
        let mut t = ToolSession::new(
            snapshot,
            WorkScope::for_batch("b-retry", &goals),
            64,
            24_000,
        );
        t.gen.deliverable = "/project".into();
        let package = t
            .dispatch("begin_generation", &json!({"entity":"ent:greeter"}))
            .unwrap();
        let test = json!({"requirement":"req:hello", "kind":"programmatic", "label":"Greeting",
            "artifact":"tests/check.sh", "name":"test_greeting", "run":"sh tests/check.sh test_greeting"});
        let args = if name == "record_generation" {
            json!({"entity":"ent:greeter", "factHash":package["factHash"],
                "manifest":{"files":["hello.sh"], "tests":[test]}})
        } else {
            json!({"requirement":"req:hello", "files":["hello.sh"], "test":test,
                "verdict":"fail", "evidence":"Awaiting a passing execution."})
        };
        for _ in 0..3 {
            let err = t.dispatch(name, &args).unwrap_err();
            assert_ne!(err.rule, "repeated-call", "{name}: {err:?}");
            assert!(err.message.contains("does not contain"), "{name}: {err:?}");
        }
        assert!(crate::gen::Ledger::load(&t.snapshot.out)
            .requirements
            .is_empty());
        crate::vfs::write(
            "/project/tests/check.sh",
            "test_greeting() { [ \"$(sh hello.sh)\" = hello ]; }\ntest_greeting\n",
        )
        .unwrap();
        let result = t.dispatch(name, &args).unwrap();
        assert!(result.get("repeat").is_none(), "{name}: {result}");
        assert!(!crate::gen::Ledger::load(&t.snapshot.out)
            .requirements
            .is_empty());
    }
}
