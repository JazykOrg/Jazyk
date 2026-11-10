use super::select::{suite_summary, verification_next};
use super::tests::fixture;
use super::*;

#[test]
fn suite_failure_summary_counts_unselected_cases_without_inventing_owners() {
    let mut tests = (0..25).map(|n| json!({"identity":format!("OtherSuite::case_{n}"),"status":"failed","report":"reports/other.xml"})).collect::<Vec<_>>();
    tests.push(json!({"identity":"selected","status":"passed"}));
    tests.push(json!({"identity":"skipped","status":"skipped"}));
    let summary = suite_summary(&[
        json!({"id":"run-1","task":"shared-suite","state":"failed","tests":tests}),
    ]);
    assert_eq!(summary["passedCases"], 1);
    assert_eq!(summary["failedCases"], 25);
    assert_eq!(summary["skippedCases"], 1);
    assert_eq!(summary["unsuccessfulRuns"], 1);
    assert_eq!(summary["failures"].as_array().unwrap().len(), 20);
    assert_eq!(summary["omittedFailures"], 5);
    assert_eq!(summary["failures"][0]["identity"], "OtherSuite::case_0");
    assert!(summary["failures"][0].get("owner").is_none());
    assert!(summary["failures"][0].get("requirement").is_none());
}

#[test]
fn suite_failure_summary_keeps_atomic_identities_and_counts_oversized_entries() {
    let summary = suite_summary(&[
        json!({"id":"run-1","task":"shared-suite","state":"failed","tests":[
            {"identity":"x".repeat(40_000),"status":"failed","report":"reports/a.xml"},
            {"identity":"Exact::case","status":"failed","report":"reports/b.xml"}
        ]}),
    ]);
    assert_eq!(summary["failedCases"], 2);
    assert_eq!(summary["omittedFailures"], 1);
    assert_eq!(summary["failures"].as_array().unwrap().len(), 1);
    assert_eq!(summary["failures"][0]["identity"], "Exact::case");
    assert!(summary["failures"].to_string().len() < 32 * 1024);
}

#[test]
fn generated_candidate_failure_guidance_permits_guarded_repair_without_rescheduling() {
    let next = verification_next(1, 0, &[], &[], false, true);
    assert!(next.contains("generated candidate"));
    assert!(next.contains("owning entity's permitted generation tools"));
    assert!(next.contains("record_generation and run_tests"));
    assert!(next.contains("does not schedule automatic regeneration"));
    assert!(!next.contains("not a row to regenerate"));
}

#[test]
fn document_diagnostic_guidance_takes_priority_over_candidate_repair() {
    let next = verification_next(1, 0, &[], &[], true, true);
    assert!(next.contains("document author") && next.contains("do not silently rewrite prose"));
    assert!(!next.contains("record_generation"));
    let runner = verification_next(0, 1, &[], &[], false, false);
    assert!(runner.contains("never judged") && runner.contains("fix the runner"));
    let pass = verification_next(0, 0, &[json!({"verdict":"pass"})], &[], false, false);
    assert_eq!(pass, "all selected rows pass");
}

#[test]
fn explicitly_selected_unbound_requirement_cannot_report_all_passed() {
    let _scope = crate::vfs::Scope::enter(&Default::default());
    let out = std::path::Path::new("/project/.jazyk");
    let (store, gs) = fixture(out);
    let mut ledger = Ledger::load(out);
    ledger.requirements.clear();
    ledger.save(out).unwrap();
    for target in ["req:shop-1", "ent:cart"] {
        let result = run_selected(&store, &gs, &[target.into()]).unwrap();
        assert_eq!(result["passed"], 0);
        assert_eq!(result["skipped"][0]["requirement"], "req:shop-1");
        assert_ne!(result["next"], "all selected rows pass");
    }
    let empty = run_selected(&store, &gs, &["unknown".into()]).unwrap();
    assert_ne!(empty["next"], "all selected rows pass");
}

#[test]
fn hosted_programmatic_tests_remain_pending_ci_without_false_verdicts() {
    let _scope = crate::vfs::Scope::enter(&Default::default());
    let out = std::path::Path::new("/project/.jazyk");
    let (store, gs) = fixture(out);
    let before = Ledger::load(out);
    let result = run_selected(&store, &gs, &[]).unwrap();
    assert_eq!(result["pendingCi"], true);
    assert_eq!(result["passed"], 0);
    assert_eq!(result["failed"], 0);
    assert_eq!(
        serde_json::to_value(Ledger::load(out)).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    let project = crate::project::Project::load(std::path::Path::new("/project"));
    let llm = crate::llm::Llm {
        api: "chat_completions".into(),
        reasoning_summary: None,
        base_url: "must-not-connect".into(),
        model: "unused".into(),
        api_key: String::new(),
        temperature: None,
        trace: None,
    };
    let runner = crate::inprocess::InProcessRunner::start(&project, &llm, out).unwrap();
    let trace = crate::session::Trace::to_sink(
        crate::session::TraceLevel::Quiet,
        std::sync::Arc::new(|_| {}),
        Default::default(),
    );
    let report = run_all(&store, &runner, &gs, &[], None, false, &trace).unwrap();
    assert_eq!(report["verified"], 0);
    assert_eq!(report["failing"], 0);
    assert_eq!(report["pendingCi"].as_array().unwrap().len(), 1);
    assert_eq!(Ledger::load(out).requirements["req:shop-1"].verdict, "none");
}
