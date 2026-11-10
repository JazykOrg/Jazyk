use super::tests::fixture;
use super::*;
use crate::model::*;

#[test]
fn task_reruns_require_a_unique_completed_case_verdict() {
    let result = serde_json::json!({"complete":true,"exit_code":1,"tests":[
        {"identity":"A::same","status":"passed"},
        {"identity":"B::same","status":"failed"},
        {"identity":"A::skipped","status":"skipped"}
    ]});
    assert!(
        super::runners::task_case_outcome(&result, "A::same", "r1")
            .unwrap()
            .pass
    );
    assert!(
        !super::runners::task_case_outcome(&result, "B::same", "r1")
            .unwrap()
            .pass
    );
    for selector in ["same", "A::skipped", "missing"] {
        assert!(super::runners::task_case_outcome(&result, selector, "r1").is_err());
    }
    let mut incomplete = result.clone();
    incomplete["complete"] = serde_json::json!(false);
    assert!(super::runners::task_case_outcome(&incomplete, "A::same", "r1").is_err());
}

// A command that could not run has judged nothing: the row stays unverified with
// reason runner-failed, and a real verdict later clears it.
// Mirrors docs/consumers/gen.md#a-test-that-could-not-run-says-nothing.
#[test]
fn a_runner_failure_is_not_a_failing_requirement() {
    let out =
        std::env::temp_dir().join(format!("jazyk-verify-runner-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);

    record_runner_failure(&s, "req:shop-1", 127, "sh: pytest: command not found").unwrap();
    let ledger = Ledger::load(&out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(row.verdict, "none");
    assert_eq!(row.exit_code, Some(127));
    let (status, reason) = status_of(&s, "req:shop-1", row, &gs);
    assert_eq!(status, "unverified");
    assert_eq!(reason, "runner-failed");

    // A verdict from a working runner clears the mark and keeps the run's exit
    // code beside the output (docs/consumers/gen.md#runners).
    mark(
        &s,
        "req:shop-1",
        "fail",
        None,
        Some("assert failed"),
        Some(1),
        &gs,
    )
    .unwrap();
    let ledger = Ledger::load(&out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(row.exit_code, Some(1));
    assert_eq!(status_of(&s, "req:shop-1", row, &gs).0, "failing");

    // A judged verdict carries no exit code: the field stays programmatic-only.
    mark(&s, "req:shop-1", "pass", None, Some("judged"), None, &gs).unwrap();
    let ledger = Ledger::load(&out);
    assert_eq!(ledger.requirements["req:shop-1"].exit_code, None);
    crate::vfs::remove_dir_all(&out).ok();
}

// The run_tests path never records a runner failure as a failing requirement:
// exit 127 routes through record_runner_failure, the verdict stays none.
// Mirrors docs/consumers/gen.md#a-test-that-could-not-run-says-nothing.
#[test]
fn run_selected_routes_runner_failures() {
    let out = std::env::temp_dir().join(format!("jazyk-runsel-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    {
        let mut ledger = Ledger::load(&out);
        ledger.requirements.get_mut("req:shop-1").unwrap().test.run =
            "jazyk-no-such-runner-xyz".into();
        ledger.save(&out).unwrap();
    }
    let reply = run_selected(&s, &gs, &[]).unwrap();
    assert_eq!(reply["failed"], 0, "{}", reply);
    assert_eq!(reply["runnerFailed"], 1, "{}", reply);
    let ledger = Ledger::load(&out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(row.verdict, "none");
    assert_eq!(row.exit_code, Some(127));
    assert_eq!(status_of(&s, "req:shop-1", row, &gs).1, "runner-failed");
    crate::vfs::remove_dir_all(&out).ok();
}

// A runner failure on the headline clears the criterion entry that is the
// headline's test: a stale verdict there must not outlive the failed run
// (docs/consumers/gen.md#a-test-that-could-not-run-says-nothing).
#[test]
fn headline_runner_failure_clears_its_criterion_entry() {
    let out = std::env::temp_dir().join(format!(
        "jazyk-headline-runner-{}",
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut store, gs) = fixture(&out);
    store.graph.criteria.insert(
        "c:check".into(),
        Criterion {
            requirement: "req:shop-1".into(),
            statement: "Check items".into(),
            ..Default::default()
        },
    );
    let mut ledger = Ledger::load(&out);
    let row = ledger.requirements.get_mut("req:shop-1").unwrap();
    row.tests.insert(
        "c:check".into(),
        crate::gen::CritTest {
            test: row.test.clone(),
            criterion_hash: hash_hex("Check items"),
            test_hash: row.hashes.test.clone(),
            verdict: "fail".into(),
            last_run: None,
            exit_code: Some(1),
            evidence: Some("earlier fail".into()),
        },
    );
    ledger.save(&out).unwrap();
    record_runner_failure(&store, "req:shop-1", 127, "sh: not found").unwrap();
    let ledger = Ledger::load(&out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(row.tests["c:check"].verdict, "none");
    assert_eq!(row.tests["c:check"].exit_code, Some(127));
    assert_eq!(status_of(&store, "req:shop-1", row, &gs).1, "runner-failed");
    crate::vfs::remove_dir_all(&out).ok();
}

// A criterion test sharing the headline's command shares its execution and gets
// its verdict (docs/consumers/gen.md#test-artifact-selection).
#[test]
fn criterion_sharing_the_headline_command_gets_a_verdict() {
    let out =
        std::env::temp_dir().join(format!("jazyk-shared-command-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut store, gs) = fixture(&out);
    store.graph.criteria.insert(
        "c:other".into(),
        Criterion {
            requirement: "req:shop-1".into(),
            statement: "Other check".into(),
            ..Default::default()
        },
    );
    let artifact = gs.deliverable.join("tests/cart.rs");
    let text = crate::vfs::read_to_string(&artifact).unwrap();
    crate::vfs::write(&artifact, format!("{text}fn criterion_other() {{}}\n")).unwrap();
    let run = "echo ran >> runs.txt";
    let mut ledger = Ledger::load(&out);
    let row = ledger.requirements.get_mut("req:shop-1").unwrap();
    row.test.run = run.into();
    let mut test = row.test.clone();
    test.name = "criterion_other".into();
    row.tests.insert(
        "c:other".into(),
        crate::gen::CritTest {
            test,
            criterion_hash: hash_hex("Other check"),
            test_hash: String::new(),
            verdict: "none".into(),
            last_run: None,
            exit_code: None,
            evidence: None,
        },
    );
    ledger.save(&out).unwrap();
    let reply = run_selected(&store, &gs, &[]).unwrap();
    assert_eq!(reply["passed"], 1, "{reply}");
    let ledger = Ledger::load(&out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(row.tests["c:other"].verdict, "pass");
    assert_eq!(status_of(&store, "req:shop-1", row, &gs).0, "verified");
    let runs = crate::vfs::read_to_string(gs.deliverable.join("runs.txt")).unwrap();
    assert_eq!(runs.lines().count(), 1, "one shared execution");
    crate::vfs::remove_dir_all(&out).ok();
}

// A run whose verdict cannot be recorded is not counted as a pass.
#[test]
fn an_unrecorded_verdict_is_not_reported_as_passed() {
    let out = std::env::temp_dir().join(format!("jazyk-unrecorded-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (store, gs) = fixture(&out);
    let mut ledger = Ledger::load(&out);
    ledger.requirements.get_mut("req:shop-1").unwrap().test.run =
        format!("rm -rf '{}'", out.join("gen/requirements").display());
    ledger.save(&out).unwrap();
    let reply = run_selected(&store, &gs, &[]).unwrap();
    assert_eq!(reply["passed"], 0, "{reply}");
    let reason = reply["skipped"][0]["reason"].as_str().unwrap_or_default();
    assert!(reason.contains("not recorded"), "{reply}");
    crate::vfs::remove_dir_all(&out).ok();
}

#[test]
fn criterion_runner_failures_clear_previous_passing_evidence() {
    let out = std::env::temp_dir().join(format!(
        "jazyk-criterion-runner-{}",
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut store, gs) = fixture(&out);
    store.graph.criteria.insert(
        "c:check".into(),
        Criterion {
            requirement: "req:shop-1".into(),
            statement: "Check items".into(),
            ..Default::default()
        },
    );
    for (command, cwd, exit_code) in [
        ("jazyk-absent-criterion-runner", ".", Some(127)),
        ("printf checked", "missing-directory", None),
    ] {
        let mut ledger = Ledger::load(&out);
        let row = ledger.requirements.get_mut("req:shop-1").unwrap();
        row.verdict = "pass".into();
        let mut test = row.test.clone();
        test.run = command.into();
        test.cwd = cwd.into();
        row.tests.insert(
            "c:check".into(),
            crate::gen::CritTest {
                test,
                criterion_hash: hash_hex("Check items"),
                test_hash: row.hashes.test.clone(),
                verdict: "pass".into(),
                last_run: None,
                exit_code: Some(0),
                evidence: Some("earlier pass".into()),
            },
        );
        assert_eq!(status_of(&store, "req:shop-1", row, &gs).0, "verified");
        ledger.save(&out).unwrap();
        let reply = run_criterion_tests(&store, "req:shop-1", &gs);
        assert_eq!(reply[0]["verdict"], "none", "{reply:?}");
        let ledger = Ledger::load(&out);
        let row = &ledger.requirements["req:shop-1"];
        let test = &row.tests["c:check"];
        assert_eq!(test.verdict, "none");
        assert_eq!(test.exit_code, exit_code);
        assert!(test.last_run.is_some());
        assert_ne!(test.evidence.as_deref(), Some("earlier pass"));
        assert_eq!(status_of(&store, "req:shop-1", row, &gs).0, "unverified");
    }
    crate::vfs::remove_dir_all(&out).ok();
}

// A criterion test renamed or deleted by hand never reads verified: the runner
// greps for the name first, and derivation reads the row `missing`
// (docs/consumers/gen.md#status-is-derived-never-stored).
#[test]
fn renamed_criterion_test_reads_artifact_gone() {
    let out = std::env::temp_dir().join(format!(
        "jazyk-criterion-renamed-{}",
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut store, gs) = fixture(&out);
    store.graph.criteria.insert(
        "c:check".into(),
        Criterion {
            requirement: "req:shop-1".into(),
            statement: "Check items".into(),
            ..Default::default()
        },
    );
    let mut ledger = Ledger::load(&out);
    let row = ledger.requirements.get_mut("req:shop-1").unwrap();
    row.verdict = "pass".into();
    let mut test = row.test.clone();
    test.name = "criterion_case_renamed_away".into();
    test.run = "exit 0".into();
    test.label = "criterion".into();
    row.tests.insert(
        "c:check".into(),
        crate::gen::CritTest {
            test,
            criterion_hash: hash_hex("Check items"),
            test_hash: row.hashes.test.clone(),
            verdict: "pass".into(),
            last_run: None,
            exit_code: Some(0),
            evidence: Some("earlier pass".into()),
        },
    );
    assert_eq!(
        status_of(&store, "req:shop-1", row, &gs),
        ("missing".into(), "artifact-gone".into())
    );
    ledger.save(&out).unwrap();
    let reply = run_criterion_tests(&store, "req:shop-1", &gs);
    let line = reply.iter().find(|l| l["criterion"] == "c:check").unwrap();
    assert_eq!(line["verdict"], "none", "{reply:?}");
    let ledger = Ledger::load(&out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(row.tests["c:check"].verdict, "none");
    assert_eq!(
        status_of(&store, "req:shop-1", row, &gs),
        ("missing".into(), "artifact-gone".into())
    );
    crate::vfs::remove_dir_all(&out).ok();
}

// A programmatic artifact that lost its declared test name judges nothing: the
// row is missing (artifact-gone), bind work, and never a verify goal that a
// rerun could not resolve. Mirrors docs/consumers/gen.md#status-is-derived-never-stored.
#[test]
fn a_renamed_test_is_bind_work_not_a_rerun() {
    let out = std::env::temp_dir().join(format!("jazyk-name-lost-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    mark(&s, "req:shop-1", "pass", None, Some("ok"), Some(0), &gs).unwrap();
    crate::vfs::write(gs.deliverable.join("tests/cart.rs"), "fn renamed() {}\n").unwrap();
    let ledger = Ledger::load(&out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(
        status_of(&s, "req:shop-1", row, &gs),
        ("missing".to_string(), "artifact-gone".to_string())
    );
    let owed = crate::bind::pending(&s, &gs);
    assert_eq!(owed.len(), 1);
    assert_eq!(owed[0]["reason"], "artifact-gone");
    assert!(crate::gen::ledger_stale_records(&s, &gs)
        .iter()
        .all(|c| c.detail["goal"] != "verify"));
    let err = match run_programmatic(&s, "req:shop-1", &gs) {
        Ok(_) => panic!("a renamed test must not execute"),
        Err(e) => e,
    };
    assert!(
        err.contains("artifact-gone") && err.contains("jazyk gen ent:cart"),
        "{}",
        err
    );
    crate::vfs::remove_dir_all(&out).ok();
}
