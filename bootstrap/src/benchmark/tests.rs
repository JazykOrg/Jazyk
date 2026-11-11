use super::*;
use crate::model::*;
use crate::session::{Trace, TraceLevel};
use crate::vfs::VPathExt;
use serde_json::Value;
use std::collections::BTreeMap;

#[test]
fn judge_measurement_uses_replies_without_parent_token_accounting() {
    use crate::session::TraceEvent;
    use std::sync::atomic::Ordering;
    let (trace, replies) = judge_completion_trace(&Trace::stderr(TraceLevel::Quiet));
    trace.event(TraceEvent::VerifyRowError {
        requirement: "req:test".into(),
        message: "provider unavailable".into(),
    });
    assert_eq!(replies.load(Ordering::Relaxed), 0);
    trace.event(TraceEvent::ModelText {
        summary: false,
        label: "judge".into(),
        text: "FAIL: wrong output".into(),
        reply: true,
    });
    assert_eq!(replies.load(Ordering::Relaxed), 1);
    trace.event(TraceEvent::ModelText {
        summary: false,
        label: "judge".into(),
        text: "not a parseable verdict".into(),
        reply: true,
    });
    assert_eq!(
        replies.load(Ordering::Relaxed),
        2,
        "an invalid judgment was still received"
    );
}

#[test]
fn runtime_fixture_is_a_real_project_with_private_state() {
    let cases = parse_cases();
    let case = cases.iter().find(|c| c.name == "gen-basic").unwrap();
    let root = own_dir("runtime-project");
    prepare_runtime_fixture(case, &root).unwrap();
    let store = sandbox(case, &root);
    crate::vfs::create_dir_all(&store.out).unwrap();
    crate::vfs::write(store.out.join("private.yaml"), "state").unwrap();
    assert_eq!(crate::task_runtime::project_root(&store.out).unwrap(), root);
    let workspace = crate::document_sets::Workspace::load(&root).unwrap();
    let project = workspace.resolve(Some("ds_main")).unwrap();
    assert_eq!(project.out, store.out);
    assert_eq!(crate::gen::GenSettings::resolve(&project).deliverable, root);
    let gs = fixture_gen_settings(&root).unwrap();
    let item = crate::model::WorkItem {
        task: case.task_type.clone(),
        target: case.target.clone(),
        dirty_sections: Vec::new(),
        stale_anchors: Vec::new(),
        proposals: Vec::new(),
    };
    let mut worker = case_session(store.clone(), &item, &case.lint, &gs, None).session;
    worker
        .dispatch(
            "begin_generation",
            &serde_json::json!({"entity":"ent:greeter"}),
        )
        .unwrap();
    worker
        .dispatch(
            "write_text_file",
            &serde_json::json!({"path":"hello.sh","content":"echo hello"}),
        )
        .unwrap();
    assert!(root.join("hello.sh").exists());
    assert!(worker
        .dispatch(
            "write_text_file",
            &serde_json::json!({"path":"docs/greeter.md","content":"replace docs"})
        )
        .is_err());
    let inputs = crate::task_runtime::snapshot(&root).unwrap();
    assert!(inputs.contains_key(".jazyk/jazyk.toml"));
    assert!(inputs.contains_key("docs/greeter.md"));
    assert!(!inputs.contains_key(".jazyk/ds_main/private.yaml"));
    crate::vfs::remove_dir_all(root).ok();
}

// A generate and a bind case read the contract texts an owned worker reads: the
// generation contract once, after the prompt, and begin_generation without it;
// the execution contract once in the prompt and begin_binding with the binding
// contract alone. Mirrors docs/benchmark/benchmark.md#runs.
#[test]
fn deliverable_cases_read_the_contracts_live_runs_read() {
    let cases = parse_cases();
    let case = cases.iter().find(|c| c.name == "gen-basic").unwrap();
    let root = own_dir("opening-contract-gen");
    prepare_runtime_fixture(case, &root).unwrap();
    let store = sandbox(case, &root);
    let gs = fixture_gen_settings(&root).unwrap();
    let item = crate::model::WorkItem {
        task: case.task_type.clone(),
        target: case.target.clone(),
        dirty_sections: Vec::new(),
        stale_anchors: Vec::new(),
        proposals: Vec::new(),
    };
    let cs = case_session(store, &item, &case.lint, &gs, None);
    let contract = crate::gen::contract();
    assert_eq!(cs.prompt.matches(contract.as_str()).count(), 1);
    assert_eq!(cs.prompt.matches(crate::task_runtime::PROMPT).count(), 1);
    let mut worker = cs.session;
    let reply = worker
        .dispatch(
            "begin_generation",
            &serde_json::json!({"entity": "ent:greeter"}),
        )
        .unwrap();
    assert!(reply.get("instructions").is_none(), "{}", reply);
    assert!(reply["factHash"].is_string(), "{}", reply);
    crate::vfs::remove_dir_all(&root).ok();

    let case = cases.iter().find(|c| c.name == "bind-descriptor").unwrap();
    let tmp = own_dir("opening-contract-bind");
    let (store, goal) = seed_derived(case, &tmp).unwrap();
    let gs = fixture_gen_settings(&tmp).unwrap();
    let item = crate::model::WorkItem::from_goal(&goal);
    let cs = case_session(store, &item, &case.lint, &gs, Some(&goal));
    assert!(!cs.prompt.contains(contract.as_str()));
    assert_eq!(cs.prompt.matches(crate::task_runtime::PROMPT).count(), 1);
    let mut worker = cs.session;
    let reply = worker
        .dispatch(
            "begin_binding",
            &serde_json::json!({"requirement": goal.target}),
        )
        .unwrap();
    assert_eq!(reply["instructions"], crate::bind::contract(), "{}", reply);
    crate::vfs::remove_dir_all(&tmp).ok();
}

// A case session runs in process with its batch open and no begin_goals among its
// tools, so its prompt never tells the model to claim it
// (docs/compiler/sessions.md#the-prompt).
#[test]
fn a_case_session_prompt_carries_the_open_batch_protocol() {
    let cases = parse_cases();
    let case = cases.iter().find(|c| c.name == "extract").unwrap();
    let tmp = own_dir("open-batch-prompt");
    let store = sandbox(case, &tmp);
    let item = crate::model::WorkItem {
        task: case.task_type.clone(),
        target: case.target.clone(),
        dirty_sections: store
            .docs
            .get(&case.target)
            .map(|r| r.sections.keys().cloned().collect())
            .unwrap_or_default(),
        stale_anchors: Vec::new(),
        proposals: Vec::new(),
    };
    let gs = crate::gen::GenSettings {
        configured: true,
        deliverable: tmp.to_path_buf(),
        worker: "agentic".into(),
        code: vec!["**".into()],
    };
    let cs = case_session(store, &item, &case.lint, &gs, None);
    assert!(cs.prompt.contains("which is already open"), "{}", cs.prompt);
    assert!(
        !cs.prompt.contains("First call `begin_goals`"),
        "{}",
        cs.prompt
    );
    assert!(!cs.tools.iter().any(|t| t.name == "begin_goals"));
    crate::vfs::remove_dir_all(&tmp).ok();
}

// A reconcile-doc case lists the sections a build would: the code block follows
// its holding section, so the batch never asks for a mark set_coverage stores on
// the holder. Mirrors docs/benchmark/cases.md#case-format.
#[test]
fn a_case_batch_lists_the_sections_that_owe_a_mark() {
    let cases = parse_cases();
    let case = cases.iter().find(|c| c.name == "steps").unwrap();
    let tmp = own_dir("dirty-steps");
    let store = sandbox(case, &tmp);
    let dirty = case_dirty_sections(&store, case);
    assert!(dirty.iter().any(|s| s == "/dedupe/steps"), "{:?}", dirty);
    assert!(
        !dirty.iter().any(|s| s.contains("code-block")),
        "{:?}",
        dirty
    );
    crate::vfs::remove_dir_all(&tmp).ok();
}

#[test]
fn parses_all_embedded_cases() {
    let cases = parse_cases();
    // Thirty-two files; review, verify-judge, rejudge-pair, dedupe-candidates,
    // abstract-entity, and declare-edges hold two blocks each.
    assert_eq!(cases.len(), 38);
    // The new tiers parse with their pars and fixtures.
    let gen = cases.iter().find(|c| c.name == "gen-basic").unwrap();
    assert_eq!(gen.tier, "generation");
    assert_eq!(gen.task_type, "generate-entity");
    assert_eq!(gen.par_rounds, 10);
    // The Node conventions case is a generation of the same kind, with no external
    // dependency so its recorded commands run offline.
    let node = cases.iter().find(|c| c.name == "gen-node").unwrap();
    assert_eq!(node.tier, "generation");
    assert_eq!(node.task_type, "generate-entity");
    assert_eq!(node.requirements.len(), 4);
    assert_eq!(node.checks.len(), 10);
    assert!(node.deliverable.is_empty());
    assert!(node.checks.iter().any(|(k, _)| k == "fileCount"));
    assert_eq!(
        node.checks
            .iter()
            .filter(|(k, _)| k == "fileMatches")
            .count(),
        5
    );
    let steps = cases.iter().find(|c| c.name == "steps").unwrap();
    assert_eq!(steps.checks.len(), 5); // an inner fence must not sever the asserts
    assert!(
        steps.docs["docs/dedupe.md"].contains("remember the line"),
        "fixture doc truncated"
    );
    let vj = cases.iter().filter(|c| c.tier == "verification").count();
    assert_eq!(vj, 2);
    let vp = cases
        .iter()
        .find(|c| c.name == "verify-judge-pass")
        .unwrap();
    assert!(!vp.deliverable.is_empty());
    assert!(cases.iter().any(|c| c.name == "declarative"));
    assert!(cases.iter().any(|c| c.name == "review-clean"));
    let extract = cases.iter().find(|c| c.name == "extract").unwrap();
    assert_eq!(extract.task_type, "reconcile-doc");
    assert_eq!(extract.checks.len(), 6);
    // Tier defaults to extraction; the review and structure cases declare theirs.
    assert_eq!(extract.tier, "extraction");
    assert_eq!(cases.iter().filter(|c| c.tier == "review").count(), 11);
    assert_eq!(cases.iter().filter(|c| c.tier == "structure").count(), 6);
    let lint = cases.iter().find(|c| c.name == "review-lint").unwrap();
    assert_eq!(lint.lint.warnings.len(), 1);
    // The derived-goal kinds keep their kind as the task and carry their views.
    let fan = cases.iter().find(|c| c.name == "abstract-entity").unwrap();
    assert!(fan.derives_goal());
    assert_eq!(fan.kind, "abstract-entity");
    assert_eq!(fan.task_type, "abstract-entity");
    assert_eq!(fan.entities.len(), 12);
    assert_eq!(fan.checks.len(), 9);
    let split = cases.iter().find(|c| c.name == "split-view").unwrap();
    assert_eq!(split.views.len(), 1);
    assert!(!extract.derives_goal());
    assert!(!cases
        .iter()
        .find(|c| c.name == "review")
        .unwrap()
        .derives_goal());
    // Every embedded pattern must compile, or a case is unwinnable.
    for case in &cases {
        for (kind, arg) in &case.checks {
            let pat = match kind.as_str() {
                "entityAbsent" | "entityNameCount" | "groupingOf" => arg["namePattern"].as_str(),
                "requirementExists" | "facetCount" => arg["statementPattern"].as_str(),
                "viewExists" => arg["titlePattern"].as_str(),
                "fileMatches" => arg["pattern"].as_str(),
                "fileCount" => arg["pathPattern"].as_str(),
                _ => None,
            };
            if let Some(pat) = pat {
                assert!(compile(pat).is_ok(), "{}: {}", case.name, pat);
            }
            if let Some(pat) = arg["excludePattern"].as_str() {
                assert!(compile(pat).is_ok(), "{}: {}", case.name, pat);
            }
        }
    }
}

pub(super) fn own_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "jazyk-bench-test-{}-{}",
        crate::sys::process_id(),
        name
    ));
    crate::vfs::remove_dir_all(&d).ok();
    d
}

// Every derived-goal fixture seeds through the commit path and derives exactly
// the goal its case names, with the change a build would compute.
// Mirrors docs/benchmark/cases.md#derived-goals.
#[test]
fn derived_cases_seed_and_derive_their_goal() {
    let cases = parse_cases();
    let derived: Vec<&Case> = cases.iter().filter(|c| c.derives_goal()).collect();
    assert_eq!(derived.len(), 14);
    for case in derived {
        let tmp = own_dir(&case.name);
        let (store, goal) =
            seed_derived(case, &tmp).unwrap_or_else(|e| panic!("{}: {}", case.name, e));
        assert_eq!(goal.kind, case.kind, "{}", case.name);
        assert_eq!(goal.target, case.target, "{}", case.name);
        // Every fixture quote locates, or the case is unwinnable.
        for (id, r) in &store.graph.requirements {
            let s = r.source.as_ref().unwrap();
            assert!(
                store.quote_locates(&s.doc, &s.section, &s.quote),
                "{}: {} quote does not locate",
                case.name,
                id
            );
        }
        for (id, e) in &store.graph.entities {
            for m in &e.mentions {
                assert!(
                    store.quote_locates(&m.doc, &m.section, &m.quote),
                    "{}: {} mention does not locate",
                    case.name,
                    id
                );
            }
        }
        // No check is satisfied by the untouched fixture where the case expects
        // work: the planted trap is real.
        let vacuous = case
            .checks
            .iter()
            .all(|(kind, arg)| eval_check(kind, arg, &store, 0).is_none());
        let expects_no_work =
            case.name == "dedupe-candidates-separate" || case.name == "declare-edges-none";
        assert_eq!(
            !vacuous, !expects_no_work,
            "{}: vacuous={}",
            case.name, vacuous
        );
        match case.name.as_str() {
            "abstract-entity" => {
                assert_eq!(goal.change["fan_out"], 12);
                assert!(!goal.change["candidates"].as_array().unwrap().is_empty());
                assert!(
                    goal.hints
                        .iter()
                        .any(|h| h.starts_with("namesake ent:billing ")),
                    "{:?}",
                    goal.hints
                );
            }
            "abstract-entity-namesake" => {
                assert_eq!(goal.change["fan_out"], 13);
                assert!(
                    goal.hints
                        .iter()
                        .any(|h| h.starts_with("namesake ent:checkout ")),
                    "{:?}",
                    goal.hints
                );
            }
            "split-view" => {
                assert_eq!(
                    goal.change["limits"][0]["limit"],
                    "participants-per-sequence-view"
                );
                assert!(
                    goal.hints
                        .iter()
                        .any(|h| h.starts_with("break before req:purchase-7")),
                    "{:?}",
                    goal.hints
                );
            }
            "curate-view" => {
                let matched: Vec<String> = goal.change["matched"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|m| m["id"].as_str().map(String::from))
                    .collect();
                assert!(
                    matched.contains(&"ent:refund".to_string()),
                    "{:?}",
                    goal.change
                );
                assert!(
                    matched.contains(&"ent:order-1042".to_string()),
                    "{:?}",
                    goal.change
                );
            }
            "declare-edges" => {
                assert_eq!(goal.change["entities"].as_array().unwrap().len(), 3);
            }
            "dedupe-candidates" | "dedupe-candidates-separate" => {
                assert!(goal.change["score"].as_f64().unwrap() >= 0.5);
            }
            "rejudge-pair-contradiction" | "rejudge-pair-duplicate" => {
                assert!(goal.mandatory);
            }
            "tech-constraint" => {
                // The self-reporting server and its passing test sit in the
                // deliverable, with the seeded criterion to bind.
                assert_eq!(goal.change["criteria"], 1, "{:?}", goal.change);
                assert!(tmp.join("src/server.js").vexists());
                assert!(tmp.join("test/backend.test.js").vexists());
            }
            "bind-descriptor" => {
                // The seeded criterion is what the test binds to, and the
                // descriptor and its passing test sit in the deliverable.
                assert_eq!(goal.change["criteria"], 1, "{:?}", goal.change);
                assert_eq!(goal.change["reason"], "unbound");
                assert!(tmp.join("src/backend.js").vexists());
                assert!(tmp.join("test/backend.test.js").vexists());
            }
            _ => {}
        }
        crate::vfs::remove_dir_all(&tmp).ok();
    }
}

// The narrative and containment fixtures: every seeded quote locates, every
// graded section exists, and the untouched fixture passes no check but the
// absences. Mirrors docs/benchmark/cases.md#case-format.
#[test]
fn narrative_fixtures_locate_and_grade_work() {
    let cases = parse_cases();
    for name in [
        "narrative-behavior",
        "machine-subject",
        "stated-containment",
    ] {
        let case = cases.iter().find(|c| c.name == name).unwrap();
        let tmp = own_dir(name);
        let s = sandbox(case, &tmp);
        for (id, e) in &s.graph.entities {
            for m in &e.mentions {
                assert!(
                    s.quote_locates(&m.doc, &m.section, &m.quote),
                    "{}: {}",
                    name,
                    id
                );
            }
        }
        for full in case.coverage.keys() {
            let (doc, section) = split_section_ref(full).unwrap();
            assert!(
                s.docs[&doc].sections.contains_key(&section),
                "{}: {}",
                name,
                full
            );
        }
        assert!(!case_dirty_sections(&s, case).is_empty(), "{}", name);
        for (kind, arg) in &case.checks {
            if kind == "coverageSet" {
                let (doc, section) = split_section_ref(arg["section"].as_str().unwrap()).unwrap();
                assert!(
                    s.docs[&doc].sections.contains_key(&section),
                    "{}: {}",
                    name,
                    section
                );
            }
            let absence = kind == "entityAbsent"
                || kind == "entityNameCount"
                || arg["max"].as_u64() == Some(0);
            assert_eq!(
                eval_check(kind, arg, &s, 0).is_none(),
                absence,
                "{}: {} {}",
                name,
                kind,
                arg
            );
        }
    }
}

#[test]
fn results_file_updates_in_place_per_model() {
    let tmp =
        std::env::temp_dir().join(format!("jazyk-bench-results-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&tmp).ok();
    write_results(
        &tmp,
        "model-a",
        &[("native".into(), serde_json::json!({"verdict": "review"}))],
    );
    write_results(
        &tmp,
        "model-b",
        &[("text".into(), serde_json::json!({"verdict": "extraction"}))],
    );
    write_results(
        &tmp,
        "model-a",
        &[(
            "native".into(),
            serde_json::json!({"verdict": "extraction"}),
        )],
    );
    let s = crate::vfs::read_to_string(crate::layout::benchmark_dir(&tmp).join("results.yaml"))
        .unwrap();
    let all: BTreeMap<String, Value> = serde_norway::from_str(&s).unwrap();
    assert_eq!(all.len(), 2);
    // The re-grade replaced model-a's entry; model-b survived untouched.
    assert_eq!(all["model-a"]["codecs"]["native"]["verdict"], "extraction");
    assert_eq!(all["model-b"]["codecs"]["text"]["verdict"], "extraction");
    assert_eq!(all["model-a"]["caseSetHash"], all["model-b"]["caseSetHash"]);
    crate::vfs::remove_dir_all(&tmp).ok();
}

#[test]
fn sandbox_seeds_fixture() {
    let cases = parse_cases();
    let converge = cases.iter().find(|c| c.name == "converge").unwrap();
    let tmp = std::env::temp_dir().join("jazyk-bench-test");
    let s = sandbox(converge, &tmp);
    assert!(s.graph.entities.contains_key("ent:cart"));
    assert!(s.graph.requirements.contains_key("req:shop-1"));
    assert_eq!(
        s.docs["docs/shop.md"].coverage["/shop/checkout"].state,
        "covered"
    );
    // The fixture's quote must locate in the parsed section, or the case is unwinnable.
    let r = &s.graph.requirements["req:shop-1"];
    assert!(s.quote_locates(
        &r.source.as_ref().unwrap().doc,
        &r.source.as_ref().unwrap().section,
        &r.source.as_ref().unwrap().quote
    ));
}
