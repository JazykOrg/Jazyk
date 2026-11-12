use super::tests::{fixture, multi_llm_fixture};
use super::*;
use crate::model::*;

#[test]
fn task_verification_package_uses_report_evidence() {
    let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
    let out = std::path::Path::new("/project/.jazyk");
    let (store, gs) = fixture(out);
    let mut ledger = Ledger::load(out);
    ledger.requirements.get_mut("req:shop-1").unwrap().test.run = "task:check".into();
    ledger.save(out).unwrap();
    let package = task(&store, "req:shop-1", &gs).unwrap();
    assert!(package["instructions"]
        .as_str()
        .unwrap()
        .contains("run_tests"));
    assert!(!package["instructions"]
        .as_str()
        .unwrap()
        .contains("Exit code 0 is a pass"));
}

// The judge contract puts the verdict on the first line; a reasoning-first reply
// is read from its conclusion. Mirrors docs/compiler/goals/verify.md#the-one-shot-form.
#[test]
fn verdict_parse_reads_the_verdict_line_then_the_conclusion() {
    assert_eq!(parse_verdict("PASS\nAll criteria met."), Some(true));
    assert_eq!(
        parse_verdict("FAIL\nThe criteria says PASS only when the color is set."),
        Some(false)
    );
    // Reasoning first, quoting the criteria, conclusion last: the old
    // first-occurrence parse read this as a fail.
    assert_eq!(
        parse_verdict(
            "The criteria says FAIL when the title is missing. It is present.\nTherefore: PASS"
        ),
        Some(true)
    );
    assert_eq!(
        parse_verdict("Checked everything. The row does not hold. FAIL"),
        Some(false)
    );
    assert_eq!(
        parse_verdict("PASS. Note the criteria would FAIL a missing title."),
        Some(true)
    );
    assert_eq!(
        parse_verdict("**PASS**\n\nEvery criterion holds."),
        Some(true)
    );
    assert_eq!(
        parse_verdict("Verdict: FAIL\nThe title is absent."),
        Some(false)
    );
    assert_eq!(parse_verdict("no verdict words here"), None);
    assert_eq!(parse_verdict(""), None);
}
// Verdict words match whole words only, so a reply mentioning a password, a
// bypass, or a failover is not read as a verdict.
#[test]
fn verdict_parse_matches_whole_words_only() {
    assert_eq!(parse_verdict("Password field is shown."), None);
    assert_eq!(parse_verdict("The bypass route is absent."), None);
    assert_eq!(parse_verdict("Failover is configured."), None);
    assert_eq!(
        parse_verdict("PASSWORD reset works, but the lockout is missing.\nFAIL"),
        Some(false)
    );
    assert_eq!(
        parse_verdict("The failover check holds and the bypass is closed. PASS"),
        Some(true)
    );
    assert_eq!(
        parse_verdict("FAIL\nThe password policy is absent, a bypass remains."),
        Some(false)
    );
}

#[test]
fn task_reports_update_each_criterion_without_refreshing_its_statement() {
    let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
    let out = std::path::Path::new("/project/.jazyk");
    let (mut store, gs) = fixture(out);
    let mut ledger = Ledger::load(out);
    let row = ledger.requirements.get_mut("req:shop-1").unwrap();
    row.test.run = "task:check".into();
    let name = row.test.name.clone();
    for (id, selector) in [
        ("c:pass", name.as_str()),
        ("c:skip", "skipped"),
        ("c:missing", "missing"),
    ] {
        store.graph.criteria.insert(
            id.into(),
            Criterion {
                requirement: "req:shop-1".into(),
                statement: id.into(),
                ..Default::default()
            },
        );
        let mut test = row.test.clone();
        test.name = selector.into();
        row.tests.insert(
            id.into(),
            crate::gen::CritTest {
                test,
                criterion_hash: hash_hex(id),
                test_hash: row.hashes.test.clone(),
                verdict: "pass".into(),
                last_run: None,
                exit_code: None,
                evidence: None,
            },
        );
    }
    ledger.save(out).unwrap();
    let task: crate::task_runtime::Task = serde_json::from_value(
        json!({"id":"check","name":"Check","description":"Check all cases","command":"mvn test"}),
    )
    .unwrap();
    let result = json!({"exit_code":0,"tests":[{"identity":format!("Suite::{name}"),"status":"passed"},{"identity":"Suite::skipped","status":"skipped"}]});
    crate::task_runtime::import_evidence(&store, &gs, &task, &result, "first");
    let ledger = Ledger::load(out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(row.verdict, "pass");
    assert_eq!(row.tests["c:skip"].verdict, "none");
    assert_eq!(row.tests["c:missing"].verdict, "none");
    assert_eq!(
        status_of(&store, "req:shop-1", row, &gs).1,
        "criteria-incomplete"
    );
    store.graph.criteria.get_mut("c:pass").unwrap().statement = "Changed obligation".into();
    crate::task_runtime::import_evidence(&store, &gs, &task, &result, "second");
    let ledger = Ledger::load(out);
    assert_eq!(
        ledger.requirements["req:shop-1"].tests["c:pass"].criterion_hash,
        hash_hex("c:pass")
    );
    assert_eq!(
        status_of(
            &store,
            "req:shop-1",
            &ledger.requirements["req:shop-1"],
            &gs
        )
        .0,
        "stale-requirement"
    );
}

fn criterion_judgments(store: &Store, values: [&str; 2]) -> Value {
    json!(["crit:first","crit:second"].into_iter().zip(values).map(|(cid,verdict)|json!({"criterion":cid,"verdict":verdict,"factHash":hash_hex(&store.graph.criteria[cid].statement),"evidence":"Inspected current source"})).collect::<Vec<_>>())
}

#[test]
fn llm_criteria_are_packaged_and_recorded_independently_and_atomically() {
    let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
    let out = std::path::Path::new("/project/.jazyk");
    let (store, gs) = multi_llm_fixture(out);
    let package = task(&store, "req:shop-1", &gs).unwrap();
    assert_eq!(package["criterionTests"].as_array().unwrap().len(), 2);
    assert!(package["criterionTests"][1]["criteria"]
        .as_str()
        .unwrap()
        .contains("crit:second"));
    let original = serde_json::to_value(Ledger::load(out)).unwrap();
    assert!(
        mark_llm(&store, "req:shop-1", "fail", None, None, None, &gs)
            .unwrap_err()
            .contains("criteria")
    );
    let valid = criterion_judgments(&store, ["fail", "fail"]);
    let mut stale = valid.clone();
    stale[1]["factHash"] = json!("old");
    for invalid in [json!([valid[0]]), json!([valid[0], valid[0]]), stale] {
        assert!(mark_llm(
            &store,
            "req:shop-1",
            "fail",
            None,
            None,
            Some(&invalid),
            &gs
        )
        .is_err());
        assert_eq!(serde_json::to_value(Ledger::load(out)).unwrap(), original);
    }
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// changed source").unwrap();
    let result = mark_llm(
        &store,
        "req:shop-1",
        "fail",
        None,
        Some("Both unmet"),
        Some(&valid),
        &gs,
    )
    .unwrap();
    assert_eq!(result["status"], "failing");
    let ledger = Ledger::load(out);
    assert!(ledger.requirements["req:shop-1"]
        .tests
        .values()
        .all(|t| t.verdict == "fail"));
    let split = criterion_judgments(&store, ["pass", "fail"]);
    assert_eq!(
        mark_llm(&store, "req:shop-1", "fail", None, None, Some(&split), &gs).unwrap()["status"],
        "failing"
    );
    let split_row = Ledger::load(out).requirements["req:shop-1"].clone();
    assert_eq!(split_row.tests["crit:first"].verdict, "pass");
    assert_eq!(split_row.tests["crit:second"].verdict, "fail");
    let green = criterion_judgments(&store, ["pass", "pass"]);
    assert_eq!(
        mark_llm(&store, "req:shop-1", "pass", None, None, Some(&green), &gs).unwrap()["status"],
        "verified"
    );
}

#[test]
fn fresh_task_report_does_not_launder_an_old_llm_verdict_after_source_changes() {
    let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
    let out = std::path::Path::new("/project/.jazyk");
    let (store, gs) = multi_llm_fixture(out);
    let mut ledger = Ledger::load(out);
    let row = ledger.requirements.get_mut("req:shop-1").unwrap();
    let test = row.tests.get_mut("crit:second").unwrap();
    test.test.kind = "programmatic".into();
    test.test.run = "task:check".into();
    test.test.name = "case".into();
    test.test.artifact = "tests/cart.rs".into();
    test.test_hash = hash_file(&artifact_path(out, &gs, &test.test));
    ledger.save(out).unwrap();
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// changed behavior").unwrap();
    let task: crate::task_runtime::Task = serde_json::from_value(
        json!({"id":"check","name":"Check","description":"Check cases","command":"pytest"}),
    )
    .unwrap();
    crate::task_runtime::import_evidence(
        &store,
        &gs,
        &task,
        &json!({"exit_code":0,"tests":[{"identity":"Suite::case","status":"passed"}]}),
        "fresh",
    );
    let ledger = Ledger::load(out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(row.tests["crit:first"].verdict, "none");
    assert_eq!(row.tests["crit:second"].verdict, "pass");
    assert_eq!(status_of(&store, "req:shop-1", row, &gs).0, "unverified");
    assert_eq!(
        mark_llm(
            &store,
            "req:shop-1",
            "pass",
            None,
            Some("Current source judged"),
            None,
            &gs
        )
        .unwrap()["status"],
        "verified"
    );
}

#[test]
fn mixed_programmatic_headline_keeps_both_guides_and_independent_evidence() {
    let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
    let out = std::path::Path::new("/project/.jazyk");
    let (store, gs) = multi_llm_fixture(out);
    let mut ledger = Ledger::load(out);
    let row = ledger.requirements.get_mut("req:shop-1").unwrap();
    let test = row.tests.get_mut("crit:second").unwrap();
    test.test.kind = "programmatic".into();
    test.test.run = "task:check".into();
    test.test.name = "case".into();
    test.test.artifact = "tests/cart.rs".into();
    crate::vfs::write(gs.deliverable.join("tests/cart.rs"), "fn case() {}\n").unwrap();
    test.test_hash = hash_file(&artifact_path(out, &gs, &test.test));
    row.test = test.test.clone();
    row.hashes.test = test.test_hash.clone();
    ledger.save(out).unwrap();
    let package = task(&store, "req:shop-1", &gs).unwrap();
    let instructions = package["instructions"].as_str().unwrap();
    assert!(instructions.contains("Run `run_tests`"));
    assert!(instructions.contains("Inspect every LLM entry"));
    // Legacy commands need the same harness-owned evidence, plus LLM guidance.
    let mut legacy = Ledger::load(out);
    legacy.requirements.get_mut("req:shop-1").unwrap().test.run = "pytest -k case".into();
    legacy.save(out).unwrap();
    let package = task(&store, "req:shop-1", &gs).unwrap();
    let instructions = package["instructions"].as_str().unwrap();
    assert!(instructions.contains("Run `run_tests`"));
    assert!(instructions.contains("cannot execute the legacy command"));
    assert!(instructions.contains("Inspect every LLM entry"));
    ledger.save(out).unwrap();
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// changed source").unwrap();
    let judgments = json!([criterion_judgments(&store, ["pass", "pass"])[0]]);
    mark_llm(
        &store,
        "req:shop-1",
        "pass",
        None,
        None,
        Some(&judgments),
        &gs,
    )
    .unwrap();
    let row = Ledger::load(out).requirements["req:shop-1"].clone();
    assert_eq!(row.verdict, "none");
    assert_eq!(row.tests["crit:first"].verdict, "pass");
    assert_eq!(row.tests["crit:second"].verdict, "none");
    assert_eq!(status_of(&store, "req:shop-1", &row, &gs).0, "unverified");
    let task = serde_json::from_value(
        json!({"id":"check","name":"Check","description":"Run cases","command":"pytest"}),
    )
    .unwrap();
    crate::task_runtime::import_evidence(
        &store,
        &gs,
        &task,
        &json!({"exit_code":0,"tests":[{"identity":"Suite::case","status":"passed"}]}),
        "fresh",
    );
    let row = Ledger::load(out).requirements["req:shop-1"].clone();
    assert_eq!(status_of(&store, "req:shop-1", &row, &gs).0, "verified");
}

#[test]
fn built_in_verifier_judges_every_llm_criterion_and_handles_inconclusive_output() {
    use std::cell::RefCell;
    struct Answers(RefCell<std::collections::VecDeque<String>>);
    impl crate::runner::SessionRunner for Answers {
        fn agent_name(&self) -> String {
            "fixture".into()
        }
        fn set_build_token(&self, _: Option<String>) {}
        fn run_item(
            &self,
            _: &crate::runner::BatchRun,
            _: &crate::session::Trace,
        ) -> crate::runner::SessionReport {
            panic!("No agent batch expected")
        }
        fn ask(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String, String> {
            panic!("Use traced asks")
        }
        fn ask_traced(
            &self,
            system: &str,
            user: &str,
            _: &str,
            _: &str,
            _: Option<&crate::session::Trace>,
        ) -> Result<String, String> {
            assert!(system.contains("Judge only"));
            assert!(user.contains("Implementing files:"));
            Ok(self
                .0
                .borrow_mut()
                .pop_front()
                .expect("Exactly one request per criterion"))
        }
    }
    let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
    let _command = crate::session_recovery::Scope::enter(None);
    let out = std::path::Path::new("/project/.jazyk");
    let (store, gs) = multi_llm_fixture(out);
    let trace = crate::session::Trace::to_sink(
        crate::session::TraceLevel::Quiet,
        std::sync::Arc::new(|_| {}),
        Default::default(),
    );
    let runner = Answers(RefCell::new(
        ["PASS first observed".into(), "FAIL second unmet".into()].into(),
    ));
    let report = run_all(&store, &runner, &gs, &[], Some("llm"), true, &trace).unwrap();
    assert_eq!(report["verified"], 0);
    assert_eq!(report["failing"], 1);
    assert!(runner.0.borrow().is_empty());
    let row = Ledger::load(out).requirements["req:shop-1"].clone();
    assert_eq!(row.tests["crit:first"].verdict, "pass");
    assert_eq!(row.tests["crit:second"].verdict, "fail");
    // A lost command reply reuses its completed result and counters. No
    // criterion judgment is admitted a second time under the same execution.
    let no_calls = Answers(RefCell::new(Default::default()));
    assert_eq!(
        run_all(&store, &no_calls, &gs, &[], Some("llm"), true, &trace).unwrap(),
        report
    );
    let _next_command = crate::session_recovery::Scope::enter(None);
    let runner = Answers(RefCell::new(["No judgment available".into()].into()));
    run_all(&store, &runner, &gs, &[], Some("llm"), true, &trace).unwrap();
    assert!(Ledger::load(out).requirements["req:shop-1"]
        .tests
        .values()
        .all(|t| t.verdict == "none"));
}

#[test]
fn a_review_transport_error_stops_before_the_next_criterion() {
    struct Broken(std::cell::Cell<usize>);
    impl crate::runner::SessionRunner for Broken {
        fn agent_name(&self) -> String {
            "broken".into()
        }
        fn set_build_token(&self, _: Option<String>) {}
        fn run_item(
            &self,
            _: &crate::runner::BatchRun,
            _: &crate::session::Trace,
        ) -> crate::runner::SessionReport {
            unreachable!()
        }
        fn ask(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String, String> {
            unreachable!()
        }
        fn ask_traced(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: Option<&crate::session::Trace>,
        ) -> Result<String, String> {
            self.0.set(self.0.get() + 1);
            Err("Review model transport closed.".into())
        }
    }
    let _vfs = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
    let out = std::path::Path::new("/project/.jazyk/review-error");
    let (store, gs) = multi_llm_fixture(out);
    let before = Ledger::load(out);
    let runner = Broken(std::cell::Cell::new(0));
    let trace = crate::session::Trace::to_sink(
        crate::session::TraceLevel::Normal,
        std::sync::Arc::new(|_| {}),
        Default::default(),
    );
    let error =
        run_without_build(&store, &runner, &gs, &[], Some("llm"), true, &trace, false).unwrap_err();
    assert_eq!(error, "Review model transport closed.");
    assert_eq!(runner.0.get(), 1);
    let after = Ledger::load(out);
    for (id, test) in &before.requirements["req:shop-1"].tests {
        assert_eq!(
            after.requirements["req:shop-1"].tests[id].verdict,
            test.verdict
        );
    }
}
