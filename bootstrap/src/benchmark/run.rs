use crate::llm::{self, Llm};
use crate::model::*;
use crate::session::{Trace, TraceLevel};
use crate::store::Store;
use crate::vfs::VPathExt;
use serde_json::Value;
use std::collections::BTreeMap;

use super::*;

pub fn run(llm: &Llm, out: &std::path::Path) -> i32 {
    run_filtered(llm, out, &[])
}

// A project copied whole into a sandbox, except what is never state: the repository,
// build output, and installed packages.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    crate::vfs::create_dir_all(to)?;
    for entry in crate::vfs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let n = name.to_string_lossy();
        if n == ".git" || n == "target" || n == "node_modules" {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        if src.vis_dir() {
            copy_tree(&src, &dst)?;
        } else {
            crate::vfs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

// `jazyk benchmark --project <dir> --goal <id>`: one goal's session from a copy of a
// real project at a real moment. The board derives in the sandbox, the goal runs with
// its derived change and hints, the commit lands in the sandbox, and the project is
// never touched. Mirrors docs/benchmark/benchmark.md#snippets-from-a-real-project.
pub fn run_goal(llm: &Llm, root: &std::path::Path, goal_id: &str, force: bool) -> i32 {
    use crate::model::GoalState;
    let tmp = std::env::temp_dir().join(format!(
        "jazyk-snippet-{}-{}",
        crate::sys::process_id(),
        crate::md::slug(goal_id)
    ));
    crate::vfs::remove_dir_all(&tmp).ok();
    if let Err(e) = copy_tree(root, &tmp) {
        eprintln!(
            include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/benchmark-run/jazyk-cannot-copy-into-a-sandbox-9610e.md")),
            root.display(),
            e
        );
        return 2;
    }
    let proj = crate::project::Project::load(&tmp);
    let mut store = Store::load(&proj.out);
    let (parsed, _) = crate::reconcile::parse_all(&proj);
    store.sync_docs(&parsed);
    // A typed command is its own approval (docs/compiler/control-plane.md): the
    // snippet releases both classes in the sandbox, so a fresh manual-mode project
    // shows its goals open instead of blocked on a release nobody can give here.
    crate::control::release(&proj, &proj.out, Some("compile"));
    crate::control::release(&proj, &proj.out, Some("generate"));
    let control = crate::control::Control::load(&proj, &proj.out);
    let board = crate::board::Board::derive(&store, &proj, &control);
    // A parked goal is the next build's work, and the snippet is that build's
    // session. Mirrors docs/benchmark/benchmark.md#snippets-from-a-real-project.
    let Some(goal) = board
        .goals
        .iter()
        .find(|g| g.id == goal_id && matches!(g.state, GoalState::Open | GoalState::Parked))
        .cloned()
    else {
        let open: Vec<&str> = board
            .goals
            .iter()
            .filter(|g| matches!(g.state, GoalState::Open))
            .map(|g| g.id.as_str())
            .take(40)
            .collect();
        let states: Vec<String> = board
            .goals
            .iter()
            .filter(|g| !matches!(g.state, GoalState::Open))
            .map(|g| format!("{} ({:?})", g.id, g.state))
            .take(12)
            .collect();
        eprintln!(
            "jazyk: `{}` is not an open goal here; open goals: {}{}",
            goal_id,
            if open.is_empty() {
                "(none)".to_string()
            } else {
                open.join(", ")
            },
            if states.is_empty() {
                String::new()
            } else {
                format!("; other goals: {}", states.join(", "))
            }
        );
        return 2;
    };
    if let Some(crate::goals::Ready::Blocked(reason)) = board.readiness.get(goal_id) {
        // Readiness is scheduling, not a precondition of the session
        // (docs/benchmark/benchmark.md): a forced snippet runs the blocked goal
        // and says what it skipped past.
        if force {
            println!(
                "forced: `{}` is not ready ({}); running it anyway",
                goal_id, reason
            );
        } else {
            eprintln!(
                "jazyk: `{}` is not ready: {} (--force runs it anyway)",
                goal_id, reason
            );
            return 2;
        }
    }
    let before: std::collections::BTreeSet<String> = board
        .goals
        .iter()
        .filter(|g| matches!(g.state, GoalState::Open))
        .map(|g| g.id.clone())
        .collect();
    let item = WorkItem::from_goal(&goal);
    let gs = crate::gen::GenSettings::resolve(&proj);
    let trace_dir = crate::layout::benchmark_dir(&proj.out).join("trace");
    crate::vfs::create_dir_all(&trace_dir).ok();
    let transcript = trace_dir.join(format!("snippet-{}.json", crate::md::slug(goal_id)));
    println!("snippet: {} in {}", goal_id, tmp.display());
    let (ops, rounds, failed, commit) = run_case_turn(
        llm,
        store.clone(),
        &item,
        &Default::default(),
        &gs,
        Some(&transcript),
        Some(&goal),
    );
    let code = match failed {
        Some(TurnFail::Abort(why)) => {
            println!("outcome: aborted: {}", why);
            1
        }
        Some(TurnFail::GoalFailed(reason)) => {
            println!("outcome: the model failed the goal: {}", reason);
            1
        }
        None => {
            println!("staged: {} mutation(s) in {} round(s)", ops.len(), rounds);
            for op in &ops {
                let v = serde_json::to_value(op).unwrap_or_default();
                println!(
                    "  - {} {}",
                    v["op"].as_str().unwrap_or("?"),
                    v["id"].as_str().or(v["keep"].as_str()).unwrap_or("")
                );
            }
            // A resolution with nothing staged still commits, as the build loop
            // does: the journal entry is what closes the goal.
            if !ops.is_empty() || !commit.resolved.is_empty() {
                store.apply(ops, &commit);
                // The build loop clears a resolved goal's change records once the
                // journal holds the resolution; the snippet does the same.
                if commit.resolved.iter().any(|r| r.goal == goal_id) {
                    let ids = board.records_of(goal_id);
                    if !ids.is_empty() {
                        store.clear_changes(&ids);
                    }
                    store.status.parked.retain(|p| p.goal.id != goal_id);
                    // A refusal is reported on stderr; the benchmark store is scratch.
                    store.write_status("parked goals").ok();
                }
            }
            let after = crate::board::Board::derive(&store, &proj, &control);
            let resolved = !after.open(goal_id);
            println!(
                "outcome: {}",
                if resolved {
                    "resolved"
                } else {
                    "the goal is still open after the commit"
                }
            );
            let opened: Vec<&str> = after
                .goals
                .iter()
                .filter(|g| matches!(g.state, GoalState::Open) && !before.contains(&g.id))
                .map(|g| g.id.as_str())
                .collect();
            println!(
                "opened: {}",
                if opened.is_empty() {
                    "(nothing)".to_string()
                } else {
                    opened.join(", ")
                }
            );
            if resolved {
                0
            } else {
                1
            }
        }
    };
    println!("transcript: {}", transcript.display());
    code
}

// `jazyk benchmark [case...]`: grade only the named cases. A filtered run is for
// iterating on one failure; it never lands in the machine-wide history.
// Mirrors docs/benchmark/benchmark.md#running-a-subset.
pub fn run_filtered(llm: &Llm, out: &std::path::Path, filter: &[String]) -> i32 {
    run_traced_filtered(llm, out, &Trace::stderr(TraceLevel::Quiet), filter)
}

// The GUI's entry: per-case lines reach the job's trace as notes, so a running grade
// shows progress instead of a spinner. Mirrors docs/frontends/gui.md#jobs (the
// `benchmark` kind); the page is jazyk-cloud docs/editor/editor.md#benchmarks.
pub fn run_traced(llm: &Llm, out: &std::path::Path, progress: &Trace) -> i32 {
    run_traced_filtered(llm, out, progress, &[])
}

pub(super) fn judge_completion_trace(
    base: &Trace,
) -> (Trace, std::sync::Arc<std::sync::atomic::AtomicU32>) {
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    };
    let replies = Arc::new(AtomicU32::new(0));
    let observed = replies.clone();
    let base = base.clone();
    let trace = Trace::to_sink(
        TraceLevel::Normal,
        Arc::new(move |event| {
            if matches!(event, crate::session::TraceEvent::ModelText { text, .. } if !text.trim().is_empty())
            {
                observed.fetch_add(1, Ordering::Relaxed);
            }
            base.event(event.clone());
        }),
        Default::default(),
    );
    (trace, replies)
}

fn run_traced_filtered(
    llm: &Llm,
    out: &std::path::Path,
    progress: &Trace,
    filter: &[String],
) -> i32 {
    let mut cases = parse_cases();
    if cases.is_empty() {
        eprintln!("jazyk: no benchmark cases parsed");
        return 2;
    }
    if !filter.is_empty() {
        let known: Vec<String> = cases.iter().map(|c| c.name.clone()).collect();
        for f in filter {
            if !known.contains(f) {
                eprintln!(
                    "jazyk: unknown case `{}`; available: {}",
                    f,
                    known.join(", ")
                );
                return 2;
            }
        }
        cases.retain(|c| filter.contains(&c.name));
    }
    let trace = Trace::stderr(TraceLevel::Quiet);
    println!("jazyk benchmark: model {} at {}", llm.model, llm.base_url);
    // One tiny completion before grading: a dead or misrouted endpoint fails one
    // probe, not every benchmark case.
    if let Err(e) = llm.chat(
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/benchmark-run/reply-with-the-single-word-ok-80284.md"
        )),
        "ok?",
        "bench preflight",
        "preflight",
    ) {
        println!(
            "
verdict: unmeasured  the endpoint never produced a completion ({})",
            e
        );
        return 2;
    }
    let mut any_usable = false;
    let mut codec_reports: Vec<(String, Value)> = Vec::new();

    {
        // Keep the stored report key compatible with historical benchmark data.
        let codec_name = "native";
        let started = std::time::Instant::now();
        let tokens_before = llm::tokens_spent();
        // Per-tier perfection (drives verdicts) and per-tier score sums (the scale).
        let mut tier_ok: BTreeMap<&str, bool> = BTreeMap::new();
        let mut tier_sum: BTreeMap<&str, (f64, usize)> = BTreeMap::new();
        let mut checks_passed = 0usize;
        let mut checks_total = 0usize;
        let mut eff_sum = 0.0f64;
        let mut eff_n = 0usize;
        let mut case_results: Vec<(String, Value)> = Vec::new();
        // A run with no model completion is unmeasured, not graded.
        // Mirrors docs/benchmark/benchmark.md#report.
        let mut any_completion = false;
        let mut first_abort: Option<String> = None;
        println!("\nharness: goose-gdk (native tools)");

        for case in &cases {
            let tmp = std::env::temp_dir().join(format!(
                "jazyk-bench-{}-{}",
                crate::sys::process_id(),
                case.name
            ));
            crate::vfs::remove_dir_all(&tmp).ok();
            // A derived-goal case seeds through a real commit and takes its goal off
            // the board; a fixture error fails the case before any turn runs.
            // Mirrors docs/benchmark/cases.md#derived-goals.
            let mut fixture_error: Option<String> = None;
            let mut derived_goal: Option<Goal> = None;
            let mut store = if case.derives_goal() {
                match seed_derived(case, &tmp) {
                    Ok((s, g)) => {
                        derived_goal = Some(g);
                        s
                    }
                    Err(e) => {
                        fixture_error = Some(e);
                        sandbox_docs(case, &tmp)
                    }
                }
            } else {
                fixture_error = prepare_runtime_fixture(case, &tmp).err();
                sandbox(case, &tmp)
            };
            let dirty = case_dirty_sections(&store, case);
            let item = match &derived_goal {
                Some(g) => WorkItem::from_goal(g),
                None => WorkItem {
                    task: case.task_type.clone(),
                    target: case.target.clone(),
                    dirty_sections: dirty,
                    stale_anchors: Vec::new(),
                    proposals: Vec::new(),
                },
            };
            let case_start = std::time::Instant::now();
            let case_tokens_before = llm::tokens_spent();
            // The generation turn writes into the disposable project root.
            let gs = fixture_gen_settings(&tmp).unwrap_or_else(|error| {
                fixture_error = Some(error);
                crate::gen::GenSettings::from_out(&store.out)
            });
            crate::vfs::create_dir_all(&gs.deliverable).ok();
            let mut fail: Option<String> = None;
            let mut aborted = false;
            let (rounds, staged) = if case.task_type == "verify-requirement" {
                // The llm-judge path: no turn, one judgment over a planted row. The
                // judgment runs through a throwaway runner against the sandbox.
                let (judge_trace, replies) = judge_completion_trace(&trace);
                match seed_verification(case, &store, &gs) {
                    Ok(()) => {
                        let sandbox_proj = crate::project::Project {
                            root: tmp.clone(),
                            out: store.out.clone(),
                            ..Default::default()
                        };
                        match crate::acp::runner::AcpRunner::start(&sandbox_proj, llm, &store.out) {
                            Ok(runner) => {
                                if let Err(e) = crate::verify::run_all(
                                    &store,
                                    &runner,
                                    &gs,
                                    std::slice::from_ref(&case.target),
                                    None,
                                    true,
                                    &judge_trace,
                                ) {
                                    fail = Some(format!(include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/benchmark-run/judge-run-failed-7edbe.md")), e));
                                }
                            }
                            Err(e) => fail = Some(format!("judge runner failed: {}", e)),
                        }
                    }
                    Err(e) => fail = Some(format!("fixture: {}", e)),
                }
                (replies.load(std::sync::atomic::Ordering::Relaxed), 0usize)
            } else if let Some(e) = fixture_error.take() {
                // A fixture the harness cannot seed measures nothing: no turn runs,
                // and the checks count as failed like an abort's.
                fail = Some(format!("fixture: {}", e));
                aborted = true;
                (0u32, 0usize)
            } else {
                let (staged_ops, rounds_n, failed, commit) = run_case_turn(
                    llm,
                    store.clone(),
                    &item,
                    &case.lint,
                    &gs,
                    Some(
                        &crate::layout::benchmark_dir(out)
                            .join("trace")
                            .join(format!("{}-{}.json", codec_name, case.name)),
                    ),
                    derived_goal.as_ref(),
                );
                let staged = staged_ops.len();
                match &failed {
                    // An aborted turn fails the case with the abort reason. Its checks
                    // are skipped and count as failed: an untouched fixture satisfying
                    // a check is not evidence. Mirrors docs/benchmark/benchmark.md#runs.
                    Some(TurnFail::Abort(why)) => {
                        fail = Some(format!("turn aborted: {}", why));
                        aborted = true;
                        if first_abort.is_none() {
                            first_abort = Some(why.clone());
                        }
                    }
                    // A goal the model marked failed fails the case with the model's
                    // reason; the checks are skipped and count as failed, as on an
                    // abort. Mirrors docs/benchmark/benchmark.md#runs.
                    Some(TurnFail::GoalFailed(reason)) => {
                        fail = Some(format!("goal marked failed: {}", reason));
                        aborted = true;
                    }
                    None => {
                        if staged > 0 {
                            // A derived goal commits as the session did, its
                            // resolution in the journal; an assembled one as the
                            // legacy loop does.
                            if derived_goal.is_some() {
                                store.apply(staged_ops, &commit);
                            } else {
                                store.apply(staged_ops, &item.commit(rounds_n, Some(0)));
                            }
                        }
                    }
                }
                (rounds_n, staged)
            };
            let case_tokens = llm::tokens_spent() - case_tokens_before;
            any_completion |= rounds > 0;
            let mut case_passed = 0usize;
            checks_total += case.checks.len();
            if !aborted {
                for (kind, arg) in &case.checks {
                    let verdict = if WORKFLOW_CHECKS.contains(&kind.as_str()) {
                        eval_workflow_check(kind, arg, &store, &gs, &case.target)
                    } else {
                        eval_check(kind, arg, &store, staged)
                    };
                    match verdict {
                        None => {
                            case_passed += 1;
                            checks_passed += 1;
                        }
                        Some(why) => {
                            if fail.is_none() {
                                fail = Some(format!("{}: {}", kind, why));
                            }
                        }
                    }
                }
            }
            // The scale: this case's score, and its efficiency against par.
            let case_score = if case.checks.is_empty() {
                0.0
            } else {
                case_passed as f64 / case.checks.len() as f64
            };
            let efficiency = if aborted {
                0.0
            } else {
                (case.par_rounds as f64 / rounds.max(1) as f64).min(1.0)
            };
            if !aborted {
                eff_sum += efficiency;
                eff_n += 1;
            }
            let e = tier_sum.entry(tier_key(&case.tier)).or_insert((0.0, 0));
            e.0 += case_score;
            e.1 += 1;
            crate::vfs::remove_dir_all(&tmp).ok();
            case_results.push((
                case.name.clone(),
                serde_json::json!({
                    "score": (case_score * 100.0).round() / 100.0,
                    "checks": format!("{}/{}", case_passed, case.checks.len()),
                    "rounds": rounds,
                    "tokens": case_tokens,
                    "parRounds": case.par_rounds,
                    "efficiency": (efficiency * 100.0).round() / 100.0,
                    "fail": fail.clone().unwrap_or_default(),
                }),
            ));
            match &fail {
                None => {
                    println!(
                        "  {:22} 1.00  ({} rounds, par {}, {} tok, {:.0}s)",
                        case.name,
                        rounds,
                        case.par_rounds,
                        case_tokens,
                        case_start.elapsed().as_secs_f32()
                    );
                    progress.line(
                        "benchmark",
                        &format!(
                            "{} {} 1.00 ({} rounds, {} tok)",
                            codec_name, case.name, rounds, case_tokens
                        ),
                    );
                }
                Some(why) => {
                    *tier_ok.entry(tier_key(&case.tier)).or_insert(true) = false;
                    println!(
                        "  {:22} {:.2}  {} ({} rounds, {:.0}s)",
                        case.name,
                        case_score,
                        why,
                        rounds,
                        case_start.elapsed().as_secs_f32()
                    );
                    progress.line(
                        "benchmark",
                        &format!("{} {} {:.2} {}", codec_name, case.name, case_score, why),
                    );
                }
            }
        }

        // No completion ever arrived: nothing was graded, so the run gets no
        // verdict and no results entry. Mirrors docs/benchmark/benchmark.md#report.
        if !any_completion {
            println!(
                "  verdict: unmeasured  no completion ever arrived ({})",
                first_abort.as_deref().unwrap_or("no turns ran")
            );
            return 2;
        }
        // Verdicts per workflow: a tier is held when every one of its cases scored 1.
        // A tier no case ran under (a filtered run) gets `-`, never a claim.
        // Mirrors docs/benchmark/benchmark.md#report and #running-a-subset.
        let ok = |t: &str| *tier_ok.get(t).unwrap_or(&true);
        let measured = |t: &str| tier_sum.get(t).map(|(_, n)| *n > 0).unwrap_or(false);
        let compilation = if !measured("extraction") {
            "-"
        } else if !ok("extraction") {
            "not-capable"
        } else if measured("review") && ok("review") {
            "review"
        } else {
            "extraction"
        };
        let generation = if !measured("generation") {
            "-"
        } else if ok("generation") {
            "capable"
        } else {
            "not-capable"
        };
        let verification = if !measured("verification") {
            "-"
        } else if ok("verification") {
            "capable"
        } else {
            "not-capable"
        };
        any_usable |= ok("extraction");
        let secs = started.elapsed().as_secs_f64();
        let tokens = llm::tokens_spent() - tokens_before;
        let throughput = if secs > 0.0 {
            tokens as f64 / secs
        } else {
            0.0
        };
        let tier_score = |t: &str| -> f64 {
            tier_sum
                .get(t)
                .map(|(sum, n)| if *n == 0 { 0.0 } else { sum / *n as f64 })
                .unwrap_or(0.0)
        };
        let efficiency = if eff_n == 0 {
            0.0
        } else {
            eff_sum / eff_n as f64
        };
        println!(
            "  verdicts: compilation {}  generation {}  verification {}",
            compilation, generation, verification
        );
        let score_str = |t: &str| {
            if measured(t) {
                format!("{:.2}", tier_score(t))
            } else {
                "-".to_string()
            }
        };
        println!(
            "  scores: extraction {}  review {}  structure {}  generation {}  verification {}  ({}/{} checks)",
            score_str("extraction"),
            score_str("review"),
            score_str("structure"),
            score_str("generation"),
            score_str("verification"),
            checks_passed,
            checks_total
        );
        println!(
            "  efficiency {:.2} (rounds vs par)  tokens {}  throughput ~{:.0} tok/s",
            efficiency, tokens, throughput
        );
        codec_reports.push((
            codec_name.to_string(),
            serde_json::json!({
                "verdicts": {"compilation": compilation, "generation": generation, "verification": verification},
                "scores": {
                    "extraction": score_json("extraction", &measured, &tier_score),
                    "review": score_json("review", &measured, &tier_score),
                    "structure": score_json("structure", &measured, &tier_score),
                    "generation": score_json("generation", &measured, &tier_score),
                    "verification": score_json("verification", &measured, &tier_score),
                },
                "checks": format!("{}/{}", checks_passed, checks_total),
                "efficiency": (efficiency * 100.0).round() / 100.0,
                "tokens": tokens,
                "throughput": throughput.round() as u64,
                "cases": case_results.iter().cloned().collect::<BTreeMap<String, Value>>(),
            }),
        ));
    }
    // An unmeasured native run never overwrites a real grade.
    // Mirrors docs/benchmark/benchmark.md#results-file.
    if codec_reports.is_empty() {
        eprintln!(include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/benchmark-run/jazyk-benchmark-measured-nothing-the-endpoint-never-e5fab.md")));
        return 2;
    }
    write_results(out, &llm.model, &codec_reports);
    // A filtered run is a debugging aid, never a capability grade.
    if filter.is_empty() {
        append_history(&llm.model, &llm.base_url, &codec_reports);
    }
    if any_usable {
        0
    } else {
        1
    }
}
