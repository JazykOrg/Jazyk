use super::*;

#[test]
fn a_session_that_recorded_nothing_is_no_generation() {
    let out = std::env::temp_dir().join(format!("jazyk-gen-landed-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    crate::vfs::create_dir_all(&gs.deliverable).unwrap();
    // A record from an earlier run stands in the ledger.
    crate::vfs::write(gs.deliverable.join("cart.rs"), "fn old() {}\n").unwrap();
    mark(
        &s,
        "ent:cart",
        None,
        &json!({"files": ["cart.rs"], "tests": []}),
        &gs,
    )
    .unwrap();
    let trace = crate::session::Trace::to_sink(
        crate::session::TraceLevel::Normal,
        std::sync::Arc::new(|_| {}),
        Default::default(),
    );
    let ids = vec!["ent:cart".to_string()];
    let idle = StubRunner {
        failed: None,
        record: false,
        stopped: Some("http error: HTTP 429 :: rate limit".into()),
        store: s.clone(),
        gs: gs.clone(),
    };
    let r = gen_session(&s, &idle, &gs, "ent:cart", &ids, &trace);
    let err = r.results[0].1.as_ref().unwrap_err();
    assert!(err.contains("429"), "{}", err);
    let quiet = StubRunner {
        failed: None,
        record: false,
        stopped: None,
        store: s.clone(),
        gs: gs.clone(),
    };
    let r = gen_session(&s, &quiet, &gs, "ent:cart", &ids, &trace);
    assert!(
        r.results[0]
            .1
            .as_ref()
            .unwrap_err()
            .contains("before recording completion"),
        "{:?}",
        r.results[0].1
    );
    let writer = StubRunner {
        failed: None,
        record: true,
        stopped: None,
        store: s.clone(),
        gs: gs.clone(),
    };
    let r = gen_session(&s, &writer, &gs, "ent:cart", &ids, &trace);
    assert_eq!(r.results[0].1.as_ref().unwrap(), &1, "{:?}", r.results[0].1);
    assert!(Ledger::load(&out).entities["cart"].recorded_at.is_some());
    crate::vfs::remove_dir_all(&out).ok();
}

#[test]
fn repeated_generation_in_the_same_timestamp_second_proves_completion() {
    let _scope = crate::vfs::Scope::enter(&BTreeMap::new());
    let out = Path::new("/project/.jazyk/same-second");
    let (store, gs) = fixture(out);
    let writer = StubRunner {
        record: true,
        failed: None,
        stopped: None,
        store: store.clone(),
        gs: gs.clone(),
    };
    let trace = crate::session::Trace::to_sink(
        crate::session::TraceLevel::Normal,
        std::sync::Arc::new(|_| {}),
        Default::default(),
    );
    let batch = crate::runner::BatchRun {
        id: "same-second".into(),
        goals: vec![],
        executor: None,
    };
    writer.run_item(&batch, &trace);
    let timestamp = "2026-10-30T12:00:00Z";
    let mut ledger = Ledger::load(out);
    ledger.entities.get_mut("cart").unwrap().recorded_at = Some(timestamp.into());
    ledger.save(out).unwrap();
    assert_eq!(ledger.entities["cart"].record_sequence, 1);

    struct SameSecondRunner(StubRunner);
    impl crate::runner::SessionRunner for SameSecondRunner {
        fn agent_name(&self) -> String {
            self.0.agent_name()
        }
        fn set_build_token(&self, token: Option<String>) {
            self.0.set_build_token(token);
        }
        fn run_item(
            &self,
            batch: &crate::runner::BatchRun,
            trace: &crate::session::Trace,
        ) -> crate::runner::SessionReport {
            let report = self.0.run_item(batch, trace);
            // Pin the display time after a real successful mark. The two
            // records deterministically share a second without sleeping.
            let mut ledger = Ledger::load(&self.0.store.out);
            ledger.entities.get_mut("cart").unwrap().recorded_at =
                Some("2026-10-30T12:00:00Z".into());
            ledger.save(&self.0.store.out).unwrap();
            report
        }
        fn ask(
            &self,
            system: &str,
            user: &str,
            label: &str,
            stage: &str,
        ) -> Result<String, String> {
            self.0.ask(system, user, label, stage)
        }
        fn ask_traced(
            &self,
            system: &str,
            user: &str,
            label: &str,
            stage: &str,
            trace: Option<&crate::session::Trace>,
        ) -> Result<String, String> {
            self.0.ask_traced(system, user, label, stage, trace)
        }
    }
    let result = gen_session(
        &store,
        &SameSecondRunner(writer),
        &gs,
        "ent:cart",
        &["ent:cart".into()],
        &trace,
    );
    assert!(result.results[0].1.is_ok(), "{:?}", result.results);
    let after = Ledger::load(out);
    assert_eq!(after.entities["cart"].record_sequence, 2);
    assert_eq!(
        after.entities["cart"].recorded_at.as_deref(),
        Some(timestamp)
    );
}

#[test]
fn old_generation_checkpoint_and_ledger_keep_their_completion_semantics() {
    let old = json!({
        "session":{"batch":{"id":"g:generate:ent:cart","goals":[],"executor":null},"generation":1,"report":null},
        "before":["2026-10-30T12:00:00Z"],"result":null
    });
    let mut state: GenSessionState = serde_json::from_value(old.clone()).unwrap();
    assert!(state.before_sequences.is_none());
    assert_eq!(serde_json::to_value(&state).unwrap(), old);
    let mut row: EntityGen = serde_json::from_value(json!({
        "factHash":"old", "recordedAt":"2026-10-30T12:00:00Z"
    }))
    .unwrap();
    assert_eq!(row.record_sequence, 0);
    assert!(serde_json::to_value(&row)
        .unwrap()
        .get("recordSequence")
        .is_none());
    row.record_sequence = 1;
    assert!(
        !state.recorded(0, Some(&row)),
        "old sessions still compare their admitted timestamps"
    );
    row.recorded_at = Some("2026-10-30T12:00:01Z".into());
    assert!(state.recorded(0, Some(&row)));
    state.before_sequences = Some(vec![Some(1)]);
    assert!(
        !state.recorded(0, Some(&row)),
        "new sessions ignore display time changes"
    );
    row.record_sequence = 2;
    assert!(state.recorded(0, Some(&row)));
}

#[test]
fn exhausted_generation_receipt_refuses_before_mutating_source_or_ledger() {
    let _scope = crate::vfs::Scope::enter(&BTreeMap::new());
    let out = Path::new("/project/.jazyk/exhausted-generation-receipt");
    let (store, gs) = fixture(out);
    let file = gs.deliverable.join("cart.rs");
    crate::vfs::write(&file, "fn cart() {}\n").unwrap();
    let manifest = json!({"files":["cart.rs"],"tests":[]});
    mark(&store, "ent:cart", None, &manifest, &gs).unwrap();
    let mut ledger = Ledger::load(out);
    ledger.entities.get_mut("cart").unwrap().record_sequence = u64::MAX;
    ledger.save(out).unwrap();
    let before = crate::vfs::read(Ledger::path(out)).unwrap();
    let source = "// req:shop-1 hash:abcdef12\nfn cart() {}\n";
    crate::vfs::write(&file, source).unwrap();
    assert!(mark(&store, "ent:cart", None, &manifest, &gs)
        .unwrap_err()
        .contains("sequence is exhausted"));
    assert_eq!(crate::vfs::read_to_string(&file).unwrap(), source);
    assert_eq!(crate::vfs::read(Ledger::path(out)).unwrap(), before);
}

#[test]
fn a_fresh_generation_manifest_wins_over_a_later_runtime_error() {
    let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
    let out = std::path::Path::new("/project/.jazyk/post-record");
    let (mut store, gs) = fixture(out);
    store.graph.entities.insert(
        "ent:other".into(),
        Entity {
            name: "Other".into(),
            ..Default::default()
        },
    );
    let trace = crate::session::Trace::to_sink(
        crate::session::TraceLevel::Normal,
        std::sync::Arc::new(|_| {}),
        Default::default(),
    );
    let writer = StubRunner {
        record: true,
        failed: Some("the model call failed: connection closed".into()),
        stopped: Some("the model call failed: connection closed".into()),
        store: store.clone(),
        gs: gs.clone(),
    };
    let result = gen_session(
        &store,
        &writer,
        &gs,
        "ent:cart",
        &["ent:cart".into()],
        &trace,
    );
    assert!(result.results[0].1.is_ok());
    assert!(result.failed.is_none());
    let mut ledger = Ledger::load(out);
    ledger.entities.get_mut("cart").unwrap().recorded_at = Some("before-partial".into());
    ledger.save(out).unwrap();
    let result = gen_session(
        &store,
        &writer,
        &gs,
        "ent:cart",
        &["ent:cart".into(), "ent:other".into()],
        &trace,
    );
    assert!(result.results[0].1.is_ok());
    assert_eq!(
        result.results[1].1.as_ref().unwrap_err(),
        "the model call failed: connection closed"
    );
    assert_eq!(
        result.failed.as_deref(),
        Some("the model call failed: connection closed")
    );
}

#[test]
fn a_generation_runtime_error_does_not_attempt_the_next_group() {
    use crate::runner::{BatchRun, SessionReport, SessionRunner};
    struct Broken(std::cell::Cell<usize>);
    impl SessionRunner for Broken {
        fn agent_name(&self) -> String {
            "broken".into()
        }
        fn set_build_token(&self, _: Option<String>) {}
        fn run_item(&self, _: &BatchRun, _: &crate::session::Trace) -> SessionReport {
            self.0.set(self.0.get() + 1);
            SessionReport {
                applied: 0,
                rounds: 1,
                tokens: 7,
                cached: 0,
                failed: Some("the model call failed: disconnected".into()),
                resolved: Vec::new(),
                stopped: Some("the model call failed: disconnected".into()),
            }
        }
        fn ask(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String, String> {
            self.0.set(self.0.get() + 1);
            Err("the model call failed: disconnected".into())
        }
        fn ask_traced(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: Option<&crate::session::Trace>,
        ) -> Result<String, String> {
            unreachable!()
        }
    }
    for worker in ["agentic", "pipeline"] {
        let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
        let out = std::path::Path::new("/project/.jazyk/generation-fatal");
        let (mut store, mut gs) = fixture(out);
        gs.worker = worker.into();
        store.graph.entities.insert(
            "ent:other".into(),
            Entity {
                name: "Other".into(),
                ..Default::default()
            },
        );
        store.graph.requirements.insert(
            "req:other-1".into(),
            Requirement {
                statement: "Other holds items.".into(),
                entities: vec!["ent:other".into()],
                ..Default::default()
            },
        );
        crate::vfs::create_dir_all(&gs.deliverable).unwrap();
        crate::vfs::write(gs.deliverable.join("cart.rs"), "fn previous() {}\n").unwrap();
        mark(
            &store,
            "ent:cart",
            None,
            &json!({"files":["cart.rs"],"tests":[]}),
            &gs,
        )
        .unwrap();
        let mut ledger = Ledger::load(out);
        ledger.medium = Some(Medium {
            form: "Rust source".into(),
            produced: "written".into(),
            toolchain: "rustc".into(),
            artifact: String::new(),
        });
        ledger.save(out).unwrap();
        let runner = Broken(std::cell::Cell::new(0));
        let trace = crate::session::Trace::to_sink(
            crate::session::TraceLevel::Normal,
            std::sync::Arc::new(|_| {}),
            Default::default(),
        );
        let result = run_selected(&store, &runner, &gs, &[], true, &trace, false).unwrap();
        assert_eq!(runner.0.get(), 1);
        assert_eq!(result["failures"], 1);
        assert_eq!(result["stopped"], "the model call failed: disconnected");
        assert!(!Ledger::load(out).entities.contains_key("other"));
    }
}

#[test]
fn an_incomplete_group_keeps_completed_work_and_skips_later_groups_and_build() {
    struct Partial {
        writer: StubRunner,
        calls: std::cell::Cell<usize>,
        stopped: Option<String>,
    }
    impl crate::runner::SessionRunner for Partial {
        fn agent_name(&self) -> String {
            "partial".into()
        }
        fn set_build_token(&self, _: Option<String>) {}
        fn run_item(
            &self,
            batch: &crate::runner::BatchRun,
            trace: &crate::session::Trace,
        ) -> crate::runner::SessionReport {
            self.calls.set(self.calls.get() + 1);
            match batch.goals[0].target.as_str() {
                "ent:cart" => self.writer.run_item(batch, trace),
                "ent:middle" => {
                    crate::vfs::write(
                        self.writer.gs.deliverable.join("index.html"),
                        "<html>partial shared page",
                    )
                    .unwrap();
                    let mut ledger = Ledger::load(&self.writer.store.out);
                    ledger.support.push("index.html".into());
                    ledger.build = Some(Build {
                        run: "printf built > build-was-run.txt".into(),
                        cwd: ".".into(),
                        produces: vec!["build-was-run.txt".into()],
                        last_run: None,
                    });
                    ledger.save(&self.writer.store.out).unwrap();
                    crate::runner::SessionReport {
                        applied: 0,
                        rounds: 24,
                        tokens: 20,
                        cached: 0,
                        failed: None,
                        resolved: vec![],
                        stopped: self.stopped.clone(),
                    }
                }
                other => panic!("later group {other} consumed a partial shared page"),
            }
        }
        fn ask(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String, String> {
            panic!("no new model request")
        }
        fn ask_traced(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: Option<&crate::session::Trace>,
        ) -> Result<String, String> {
            panic!("no new model request")
        }
    }
    for (case, (later, stopped)) in [
        (false, None),
        (true, None),
        (false, Some("the session ran out of rounds after 24".into())),
        (true, Some("the session ran out of rounds after 24".into())),
    ]
    .into_iter()
    .enumerate()
    {
        let _scope = crate::session_recovery::Scope::enter(None);
        let out = std::env::temp_dir().join(format!(
            "jazyk-gen-partial-stop-{}-{case}",
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&out).ok();
        let (mut store, gs) = fixture(&out);
        for (id, name) in [("middle", "Middle"), ("z-last", "Last")] {
            if id == "z-last" && !later {
                continue;
            }
            store.graph.entities.insert(
                format!("ent:{id}"),
                Entity {
                    name: name.into(),
                    ..Default::default()
                },
            );
            store.graph.requirements.insert(
                format!("req:{id}"),
                Requirement {
                    statement: format!("{name} has content."),
                    entities: vec![format!("ent:{id}")],
                    ..Default::default()
                },
            );
        }
        let mut ledger = Ledger::load(&out);
        ledger.medium = Some(Medium {
            form: "Rust source".into(),
            produced: "written".into(),
            toolchain: "rustc".into(),
            artifact: String::new(),
        });
        ledger.save(&out).unwrap();
        let runner = Partial {
            writer: StubRunner {
                record: true,
                failed: None,
                stopped: None,
                store: store.clone(),
                gs: gs.clone(),
            },
            calls: std::cell::Cell::new(0),
            stopped: stopped.clone(),
        };
        let trace = crate::session::Trace::to_sink(
            crate::session::TraceLevel::Quiet,
            std::sync::Arc::new(|_| {}),
            Default::default(),
        );
        let result = run_selected(&store, &runner, &gs, &[], true, &trace, true).unwrap();
        assert_eq!(runner.calls.get(), 2, "{result}");
        assert_eq!(result["regenerated"], 1);
        assert_eq!(result["failures"], 1);
        assert_eq!(result["build"]["state"], "not-run");
        let error = result["error"].as_str().unwrap();
        assert!(
            error.contains("group Middle") && error.contains("Still owed: Middle"),
            "{error}"
        );
        assert!(
            error.contains(stopped.as_deref().unwrap_or("before recording completion")),
            "{error}"
        );
        let ledger = Ledger::load(&out);
        assert!(ledger.entities.contains_key("cart") && !ledger.entities.contains_key("middle"));
        assert!(crate::vfs::read_to_string(gs.deliverable.join("cart.rs"))
            .unwrap()
            .contains("fn cart"));
        assert_eq!(
            crate::vfs::read_to_string(gs.deliverable.join("index.html")).unwrap(),
            "<html>partial shared page"
        );
        assert!(!gs.deliverable.join("build-was-run.txt").exists());
        assert!(ledger.build.unwrap().last_run.is_none());
        crate::vfs::remove_dir_all(&out).ok();
    }
}

#[test]
fn prior_generation_group_progress_keeps_its_position_without_an_error_field() {
    let old = json!({"root":"ent:cart","ready":["ent:cart","ent:other"],
        "pipeline_cursor":1,"pipeline_task":null,"pipeline_results":[["ent:cart",{"Ok":1}]]});
    let restored: GenGroup = serde_json::from_value(old).unwrap();
    assert_eq!(restored.pipeline_cursor, 1);
    assert_eq!(restored.pipeline_results, vec![("ent:cart".into(), Ok(1))]);
    assert!(restored.pipeline_failed.is_none());
}

#[test]
fn incomplete_generation_exposes_its_group_and_round_limit_and_stops() {
    let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
    let out = std::path::Path::new("/project/.jazyk/generation-budget");
    let (store, gs) = fixture(out);
    crate::vfs::create_dir_all(&gs.deliverable).unwrap();
    crate::vfs::write(gs.deliverable.join("cart.rs"), "fn prior() {}\n").unwrap();
    mark(
        &store,
        "ent:cart",
        None,
        &json!({"files":["cart.rs"],"tests":[]}),
        &gs,
    )
    .unwrap();
    let mut ledger = Ledger::load(out);
    ledger.medium = Some(Medium {
        form: "Rust".into(),
        produced: "written".into(),
        toolchain: "rustc".into(),
        artifact: String::new(),
    });
    ledger.save(out).unwrap();
    let runner = StubRunner {
        record: false,
        failed: None,
        stopped: Some("the session ran out of rounds after 24".into()),
        store: store.clone(),
        gs: gs.clone(),
    };
    let trace = crate::session::Trace::to_sink(
        crate::session::TraceLevel::Quiet,
        std::sync::Arc::new(|_| {}),
        Default::default(),
    );
    let result = run_selected(&store, &runner, &gs, &[], true, &trace, false).unwrap();
    assert_eq!(result["failures"], 1);
    assert_eq!(result["stopped"], result["error"]);
    let error = result["error"].as_str().unwrap();
    assert!(
        error.contains("Cart was not completed") && error.contains("24"),
        "{result}"
    );
    assert!(
        !error.contains("ent:cart") && !error.contains("record_generation"),
        "{result}"
    );
    let mut old = serde_json::to_value(GenCommand::default()).unwrap();
    old.as_object_mut().unwrap().remove("first_error");
    let restored: GenCommand = serde_json::from_value(old).unwrap();
    assert!(restored.first_error.is_none());
}

#[test]
fn a_resumed_generation_command_keeps_its_group_and_counts_the_result_once() {
    let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
    let _scope = crate::session_recovery::Scope::enter(None);
    let out = std::path::Path::new("/project/.jazyk/main");
    let (store, gs) = fixture(out);
    crate::vfs::create_dir_all(&gs.deliverable).unwrap();
    crate::vfs::write(gs.deliverable.join("cart.rs"), "fn previous() {}\n").unwrap();
    mark(
        &store,
        "ent:cart",
        None,
        &json!({"files":["cart.rs"],"tests":[]}),
        &gs,
    )
    .unwrap();
    let mut ledger = Ledger::load(out);
    ledger.entities.get_mut("cart").unwrap().recorded_at = Some("before-command".into());
    ledger.medium = Some(Medium {
        form: "Rust source".into(),
        produced: "written".into(),
        toolchain: "rustc".into(),
        artifact: String::new(),
    });
    ledger.save(out).unwrap();
    let trace = crate::session::Trace::to_sink(
        crate::session::TraceLevel::Normal,
        std::sync::Arc::new(|_| {}),
        Default::default(),
    );
    let paused = StubRunner {
        failed: None,
        record: false,
        stopped: Some("checkpoint-failed: test storage outage".into()),
        store: store.clone(),
        gs: gs.clone(),
    };
    let error = run_selected(&store, &paused, &gs, &[], true, &trace, false).unwrap_err();
    assert!(error.starts_with("checkpoint-failed:"));
    let writer = StubRunner {
        failed: None,
        record: true,
        stopped: None,
        store: store.clone(),
        gs: gs.clone(),
    };
    let result = run_selected(&store, &writer, &gs, &[], true, &trace, false).unwrap();
    assert_eq!(result["regenerated"], 1);
    assert_eq!(result["failures"], 0);
    // A worker that would interrupt cannot be reached after the result receipt.
    assert_eq!(
        run_selected(&store, &paused, &gs, &[], true, &trace, false).unwrap(),
        result
    );
}

#[test]
fn source_scaffolding_can_be_written_bound_and_recorded() {
    let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
    let root = Path::new("/project");
    crate::vfs::create_dir_all(root.join(".jazyk")).unwrap();
    crate::vfs::write(root.join(crate::document_sets::CONFIG), "version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Site'\ndocs = ['**', '!deliverable/**']\nsource = ['deliverable/**']\n").unwrap();
    let project = crate::document_sets::Workspace::load(root)
        .unwrap()
        .resolve(Some("ds_main"))
        .unwrap();
    let (store, _) = fixture(&project.out);
    let gs = GenSettings::resolve(&project);
    let mut tools = crate::tools::ToolSession::new(
        store.clone(),
        crate::tools::WorkScope::serving("generate"),
        64,
        24_000,
    );
    tools.gen = gs.clone();
    let workflow = "deliverable/.github/workflows/deploy.yml";
    let ignore = "deliverable/.gitignore";
    let artifact = "deliverable/tests/hosting.py";
    for (path, content) in [
        (
            workflow,
            "name: Deploy\non:\n  push:\n    branches: [master]\njobs: {}\n",
        ),
        (ignore, "build/\n"),
        (artifact, "def test_workflow():\n    pass\n"),
    ] {
        tools
            .dispatch("write_text_file", &json!({"path":path,"content":content}))
            .unwrap();
        assert_eq!(
            tools
                .dispatch("read_text_file", &json!({"path":path}))
                .unwrap()["content"],
            content
        );
    }
    let files = tools.dispatch("list_files", &json!({})).unwrap();
    assert!(files["files"]
        .as_array()
        .unwrap()
        .contains(&json!(workflow)));
    assert!(files["files"].as_array().unwrap().contains(&json!(ignore)));
    assert!(list_existing_files(&root.join("deliverable"))
        .contains(&".github/workflows/deploy.yml".into()));
    assert!(list_existing_files(&root.join("deliverable")).contains(&".gitignore".into()));
    assert!(crate::document_sets::Workspace::load(root)
        .unwrap()
        .files()
        .contains(&workflow.to_string()));
    let test = json!({"kind":"programmatic","artifact":artifact,"name":"test_workflow","run":"task:check"});
    crate::bind::record(
        &store,
        "req:shop-1",
        &[workflow.into()],
        &test,
        &json!([]),
        "fail",
        Some("Static fixture, no deployment executed."),
        &gs,
    )
    .unwrap();
    crate::bind::gate(&store, &gs, "req:shop-1").unwrap();
    mark(&store, "ent:cart", None, &json!({"files":[workflow],"supportFiles":[ignore],"tests":[{"requirement":"req:shop-1","kind":"programmatic","artifact":artifact,"name":"test_workflow","run":"task:check"}]}), &gs).unwrap();
    assert_eq!(gate(&store, &gs, "ent:cart", false).unwrap(), 1);
    let ledger = Ledger::load(&store.out);
    assert_eq!(ledger.entities["cart"].files, [workflow]);
    assert!(ledger.support.contains(&ignore.to_string()));
    assert!(!implementation_outside_source(
        &gs,
        &ledger,
        &ledger.entities["cart"].files
    ));
    for path in [
        "deliverable/.env",
        "deliverable/.github/.env",
        "deliverable/.ssh/config",
        "deliverable/.aws/config",
        "deliverable/.git/config",
        "deliverable/.jazyk/config",
    ] {
        let err = tools
            .dispatch(
                "write_text_file",
                &json!({"path":path,"content":"must not land"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "source-selection", "{path}");
        assert!(!root.join(path).vexists(), "{path}");
    }
    crate::vfs::create_dir_all(root.join("deliverable/nested/.jazyk")).unwrap();
    crate::vfs::write(
        root.join("deliverable/nested/.jazyk/jazyk.toml"),
        "version = 1\n",
    )
    .unwrap();
    crate::vfs::create_dir_all(root.join("deliverable/nested/.github/workflows")).unwrap();
    crate::vfs::write(
        root.join("deliverable/nested/.github/workflows/own.yml"),
        "nested project\n",
    )
    .unwrap();
    let err = tools.dispatch("write_text_file", &json!({"path":"deliverable/nested/.github/workflows/new.yml","content":"must not land"})).unwrap_err();
    assert_eq!(err.rule, "source-selection");
    let files = tools
        .dispatch("list_files", &json!({"path":"deliverable"}))
        .unwrap();
    assert!(!files["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path.as_str().unwrap().contains("nested/")));
}

#[test]
fn fresh_generation_records_criterion_tests_without_binding_and_keeps_them_unjudged() {
    let out = std::env::temp_dir().join(format!(
        "jazyk-adaptive-criteria-{}",
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut store, gs) = fixture(&out);
    for (id, statement) in [
        ("crit:items", "Two items are retained."),
        ("crit:empty", "An empty cart has no items."),
    ] {
        store.graph.criteria.insert(
            id.into(),
            crate::model::Criterion {
                requirement: "req:shop-1".into(),
                statement: statement.into(),
                kind: "positive".into(),
                ..Default::default()
            },
        );
    }
    crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
    crate::vfs::write(
        gs.deliverable.join("cart.rs"),
        "pub fn count() -> usize { 2 }\n",
    )
    .unwrap();
    crate::vfs::write(
        gs.deliverable.join("tests/cart.rs"),
        "fn items() {}\nfn empty() {}\nfn replacement() {}\n",
    )
    .unwrap();
    assert!(Ledger::load(&out).requirements.is_empty());
    assert_eq!(pending(&store, &gs)[0]["reason"], "incomplete-bindings");
    let declaration = |criterion: &str, name: &str| {
        json!({"requirement":"req:shop-1","criterion":criterion,
        "kind":"programmatic","artifact":"tests/cart.rs","name":name,"run":"task:old-check"})
    };
    let mut manifest =
        json!({"files":["cart.rs","tests/cart.rs"],"tests":[declaration("crit:items","items")]});
    mark(&store, "ent:cart", None, &manifest, &gs).unwrap();
    assert!(gate(&store, &gs, "ent:cart", false)
        .unwrap_err()
        .contains("test bindings"));
    manifest["tests"] = json!([
        declaration("crit:items", "items"),
        declaration("crit:empty", "empty")
    ]);
    mark(&store, "ent:cart", None, &manifest, &gs).unwrap();
    let ledger = Ledger::load(&out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(row.tests.len(), 2);
    assert!(row
        .tests
        .values()
        .all(|test| test.verdict == "none" && test.last_run.is_none()));
    assert_eq!(gate(&store, &gs, "ent:cart", false).unwrap(), 2);
    assert!(gate(&store, &gs, "ent:cart", true)
        .unwrap_err()
        .contains("run_tests"));
    // A replacement judge is refused before changing files or the ledger.
    let before = serde_json::to_value(&ledger).unwrap();
    let mut replaced = manifest.clone();
    replaced["tests"][0]["name"] = json!("replacement");
    assert!(mark(&store, "ent:cart", None, &replaced, &gs)
        .unwrap_err()
        .contains("replacing the judge"));
    assert_eq!(serde_json::to_value(Ledger::load(&out)).unwrap(), before);
    // Consolidating execution does not replace the judge, but loses old evidence.
    let mut ledger = Ledger::load(&out);
    let row = ledger.requirements.get_mut("req:shop-1").unwrap();
    row.verdict = "pass".into();
    row.last_run = Some("previous".into());
    for test in row.tests.values_mut() {
        test.verdict = "pass".into();
        test.last_run = Some("previous".into());
    }
    ledger.save(&out).unwrap();
    for declaration in manifest["tests"].as_array_mut().unwrap() {
        declaration["run"] = json!("task:shared-suite");
    }
    mark(&store, "ent:cart", None, &manifest, &gs).unwrap();
    let ledger = Ledger::load(&out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(row.verdict, "none");
    assert!(row
        .tests
        .values()
        .all(|test| test.test.run == "task:shared-suite"
            && test.verdict == "none"
            && test.last_run.is_none()));
    assert!(gate(&store, &gs, "ent:cart", true).is_err());
    crate::vfs::remove_dir_all(&out).unwrap();
}

#[test]
fn generated_llm_criterion_waits_for_review_without_blocking_generation() {
    let out = std::env::temp_dir().join(format!("jazyk-adaptive-llm-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut store, gs) = fixture(&out);
    store.graph.criteria.insert(
        "crit:items".into(),
        crate::model::Criterion {
            requirement: "req:shop-1".into(),
            statement: "The cart clearly shows its items.".into(),
            kind: "positive".into(),
            ..Default::default()
        },
    );
    crate::vfs::create_dir_all(gs.deliverable.join("criteria")).unwrap();
    crate::vfs::write(
        gs.deliverable.join("cart.md"),
        "The visible cart contains its selected items.\n",
    )
    .unwrap();
    let criteria = criteria_front_matter(
        "req:shop-1",
        Some("crit:items"),
        &store.graph.criteria["crit:items"].statement,
    ) + "\nInspect cart.md. PASS when its items are clear. FAIL otherwise.\n";
    crate::vfs::write(gs.deliverable.join("criteria/cart.md"), criteria).unwrap();
    mark(&store,"ent:cart",None,&json!({"files":["cart.md"],"tests":[{
        "requirement":"req:shop-1","criterion":"crit:items","kind":"llm","artifact":"criteria/cart.md","name":"clarity","run":"llm"
    }]}),&gs).unwrap();
    assert!(gate(&store, &gs, "ent:cart", true).is_ok());
    let row = &Ledger::load(&out).requirements["req:shop-1"];
    assert_eq!(row.verdict, "none");
    assert_eq!(row.tests["crit:items"].verdict, "none");
    assert_ne!(
        crate::verify::status_of(&store, "req:shop-1", row, &gs).0,
        "verified"
    );
    crate::vfs::remove_dir_all(&out).unwrap();
}

#[test]
fn gdk_develops_fresh_source_and_tests_before_execution() {
    use crate::harness::{self, GenericTool, LoopArgs, ModelHost, ModelRequest, ModelResponse};
    struct Script {
        replies: std::collections::VecDeque<Value>,
        calls: usize,
    }
    impl ModelHost for Script {
        fn infer<'a>(
            &'a mut self,
            request: ModelRequest,
        ) -> harness::LocalFuture<'a, Result<ModelResponse, String>> {
            Box::pin(async move {
                assert!(matches!(request.purpose, harness::ModelPurpose::Inference));
                self.calls += 1;
                Ok(ModelResponse {
                    body: self
                        .replies
                        .pop_front()
                        .expect("no additional model call is needed"),
                })
            })
        }
    }
    let out =
        std::env::temp_dir().join(format!("jazyk-gdk-code-first-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut store, gs) = fixture(&out);
    for (id, statement) in [
        ("crit:items", "Two items remain in the cart."),
        ("crit:empty", "An empty cart contains zero items."),
    ] {
        store.graph.criteria.insert(
            id.into(),
            crate::model::Criterion {
                requirement: "req:shop-1".into(),
                statement: statement.into(),
                kind: "positive".into(),
                ..Default::default()
            },
        );
    }
    assert!(Ledger::load(&out).requirements.is_empty());
    assert_eq!(pending(&store, &gs)[0]["reason"], "incomplete-bindings");
    let goal = crate::model::Goal {
        id: "g:generate:ent:cart".into(),
        kind: "generate".into(),
        target: "ent:cart".into(),
        mandatory: true,
        class: "compile".into(),
        ..Default::default()
    };
    let scope = crate::tools::WorkScope::for_batch(&goal.id, std::slice::from_ref(&goal));
    crate::vfs::create_dir_all(&gs.deliverable).unwrap();
    let mut session = crate::tools::ToolSession::new(store.clone(), scope, 50, 32000);
    session.gen = gs.clone();
    let source="class Cart:\n    def __init__(self, items):\n        self.items = list(items)\n    def count(self):\n        return len(self.items)\n";
    let tests="import sys\nfrom pathlib import Path\nsys.path.insert(0, str(Path(__file__).resolve().parents[1]))\nfrom cart import Cart\ndef items():\n    assert Cart(['a', 'b']).count() == 2\ndef empty():\n    assert Cart([]).count() == 0\nglobals()[sys.argv[1]]()\n";
    let manifest = json!({"files":["cart.py","tests/check.py"],"tests":[
        {"requirement":"req:shop-1","criterion":"crit:items","kind":"programmatic","artifact":"tests/check.py","name":"items","run":"python3 tests/check.py items"},
        {"requirement":"req:shop-1","criterion":"crit:empty","kind":"programmatic","artifact":"tests/check.py","name":"empty","run":"python3 tests/check.py empty"}
    ]});
    let actions = [
        (
            "write_text_file",
            json!({"path":"cart.py","content":source}),
        ),
        (
            "write_text_file",
            json!({"path":"tests/check.py","content":tests}),
        ),
        (
            "record_generation",
            json!({"entity":"ent:cart","factHash":fact_hash(&store,"ent:cart"),"manifest":manifest}),
        ),
        ("run_tests", json!({"requirements":["req:shop-1"]})),
        (
            "mark_goal_done",
            json!({"goal":goal.id,"justification":"The implementation retains items and both criterion tests passed against it."}),
        ),
        (
            "done",
            json!({"summary":"Implemented the cart and executed its criterion tests."}),
        ),
    ];
    let replies=actions.iter().enumerate().map(|(i,(name,args))|json!({
        "choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{
            "id":format!("adaptive-{i}"),"type":"function","function":{"name":name,"arguments":args.to_string()}
        }]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}
    })).collect();
    let mut model = Script { replies, calls: 0 };
    let names: std::collections::BTreeSet<_> = actions.iter().map(|(name, _)| *name).collect();
    let tools: Vec<_> = crate::tools::catalog()
        .into_iter()
        .filter(|tool| names.contains(tool.name))
        .map(|tool| GenericTool {
            name: tool.name.into(),
            description: tool.description.into(),
            parameters: tool.parameters,
        })
        .collect();
    let llm = crate::llm::Llm {
        api: "chat_completions".into(),
        reasoning_summary: None,
        base_url: "http://127.0.0.1:1/v1".into(),
        model: "scripted-native-gdk".into(),
        api_key: String::new(),
        temperature: None,
        trace: None,
    };
    let mut history = vec![
        json!({"role":"user","content":"Develop the cart source and its criterion tests together."}),
    ];
    let mut observed = Vec::new();
    let mut dispatch = |name: &str, args: &Value| {
        if name == "run_tests" {
            assert_eq!(
                crate::vfs::read_to_string(gs.deliverable.join("cart.py")).unwrap(),
                source
            );
            assert!(gs.deliverable.join("tests/check.py").vis_file());
            assert!(
                gate(&store, &gs, "ent:cart", true).is_err(),
                "declarations cannot claim execution"
            );
        }
        observed.push(name.to_string());
        let value = session
            .dispatch(name, args)
            .unwrap_or_else(|error| panic!("fixture {name}: {}", error.message));
        Ok(value.to_string())
    };
    let stopped = harness::run_with_host(
        LoopArgs {
            llm: &llm,
            history: &mut history,
            tools: &tools,
            dispatch: &mut dispatch,
            emit: &mut |_| {},
            cancelled: &|| false,
            max_rounds: 8,
            label: "code-first".into(),
            conversation: false,
            inputs: None,
            input_close: None,
        },
        &instructions(),
        &mut model,
        &mut harness::MemoryRecovery,
    );
    assert_eq!(stopped, harness::Stop::EndTurn);
    assert_eq!(
        observed,
        actions
            .iter()
            .map(|(name, _)| name.to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        model.calls, 6,
        "successful completion needs no extra inference"
    );
    assert!(gate(&store, &gs, "ent:cart", true).is_ok());
    let row = &Ledger::load(&out).requirements["req:shop-1"];
    assert_eq!(row.verdict, "pass");
    assert!(row
        .tests
        .values()
        .all(|test| test.verdict == "pass" && test.last_run.is_some()));
    assert_eq!(
        crate::verify::status_of(&store, "req:shop-1", row, &gs).0,
        "verified"
    );
    assert!(!observed
        .iter()
        .any(|name| name == "record_binding" || name == "begin_binding"));
    crate::vfs::remove_dir_all(&out).unwrap();
}
