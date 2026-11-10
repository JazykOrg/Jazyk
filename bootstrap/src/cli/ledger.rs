// Ledger commands: `decompile`, `gen`, and `test`. Mirrors docs/consumers/gen.md.
use super::{resolve, runner_for, Options};
use crate::llm;
use crate::session::{Trace, TraceLevel};
use crate::store::Store;

// Decompilation: release the named scopes and draft documents for them, or dispatch
// to an attached agent. Mirrors docs/frontends/cli.md#jazyk-decompile.
pub fn run_decompile(opts: &Options, scopes: &[String]) -> i32 {
    let (proj, llm, out) = resolve(&[], opts);
    let store = Store::load(&out);
    let gs = crate::gen::GenSettings::resolve(&proj);
    let by_scope = crate::decompile::scopes(&proj, &store, &gs);
    if by_scope.is_empty() {
        println!("jazyk: nothing unclaimed; every deliverable file is named by a binding");
        return 0;
    }
    let wanted: Vec<String> = if scopes.is_empty() {
        by_scope.keys().cloned().collect()
    } else {
        let mut v = Vec::new();
        for s in scopes {
            let s = s.trim_end_matches('/');
            if !by_scope.contains_key(s) {
                eprintln!(
                    "jazyk: no unclaimed files under `{}`; scopes with unclaimed files: {}",
                    s,
                    by_scope.keys().cloned().collect::<Vec<_>>().join(", ")
                );
                return 2;
            }
            v.push(s.to_string());
        }
        v
    };
    crate::control::release_decompile(&proj, &out, &wanted);
    // Dispatch: with an agent attached and preferred, the release is the trigger and
    // the agent's watcher does the drafting. Mirrors docs/compiler/control-plane.md#dispatch.
    let control = crate::control::Control::load(&proj, &out);
    let agents: Vec<String> = crate::control::workers(&out)
        .into_iter()
        .filter(|w| w.kind == "agent")
        .map(|w| if w.client.is_empty() { w.id } else { w.client })
        .collect();
    if control.worker != "internal" && !agents.is_empty() {
        println!(
            "jazyk: released {} scope(s) for decompilation; dispatched to agent(s): {}",
            wanted.len(),
            agents.join(", ")
        );
        return 0;
    }
    if control.worker == "agent" {
        eprintln!("jazyk: workflow.worker is `agent` but no agent is attached; connect one over `jazyk mcp decompile` or set worker to any");
        return 1;
    }
    let trace = Trace::stderr(TraceLevel::Normal).with_transcript(&out, "decompile");
    let runner = match runner_for(&proj, &llm, &out) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("jazyk: {}", e);
            return 2;
        }
    };
    let result = crate::decompile::run_all(&proj, &store, &runner, &gs, &wanted, &trace);
    match &result {
        Ok(v) => trace.finish_transcript("done", v),
        Err(e) => trace.finish_transcript("failed", &serde_json::json!({"error": e})),
    }
    match result {
        Ok(sum) => {
            println!(
                "jazyk: decompile done: {} draft session(s) landed, {} failure(s); drafts carry `unratified` until edited, `jazyk compile` extracts them",
                sum["drafted"], sum["failures"]
            );
            if sum["failures"].as_u64().unwrap_or(0) > 0 {
                1
            } else {
                0
            }
        }
        Err(e) => {
            eprintln!("jazyk: {}", e);
            1
        }
    }
}

// The built-in generation worker: consumes the same task packages an external MCP
// worker gets (gen_pending decides what runs, gen_task supplies the package, gen_mark
// records the manifest). The model owns every choice about the deliverable: the medium
// (derived from the context), the file paths, and the run commands. The harness only
// writes what the model returns, validates the manifest deterministically, and records
// it. Mirrors docs/consumers/gen.md.
pub fn run_gen(opts: &Options, entities: &[String]) -> i32 {
    let (proj, llm, out) = resolve(&[], opts);
    if opts.verbose {
        llm::set_verbose(true);
    }
    // A typed gen is an approval, and generation holds the build lease like a compile.
    let _build = match crate::control::begin_internal_build(&proj, &out, "generate") {
        Ok(g) => g,
        Err(e) => {
            eprintln!("jazyk: {}", e);
            return 1;
        }
    };
    let runner = match runner_for(&proj, &llm, &out) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("jazyk: {}", e);
            return 2;
        }
    };
    runner.set_build_token(Some(_build.token().to_owned()));
    let gs = crate::gen::GenSettings::resolve(&proj);
    // Render the worker events on the historical CLI output format.
    use crate::session::TraceEvent;
    let sink: std::sync::Arc<dyn Fn(&TraceEvent) + Send + Sync> =
        std::sync::Arc::new(|ev| match ev {
            TraceEvent::GenEntityDone { entity, files } => {
                println!("jazyk: generated {} ({} file(s))", entity, files)
            }
            TraceEvent::GenEntityFailed {
                entity,
                stage,
                error,
            } if stage == "task" => {
                eprintln!("jazyk: {}: {}", entity, error)
            }
            TraceEvent::GenEntityFailed { entity, error, .. } => {
                eprintln!("jazyk: {} failed: {}", entity, error)
            }
            _ => {}
        });
    let trace =
        Trace::to_sink(TraceLevel::Normal, sink, Default::default()).with_transcript(&out, "gen");
    // The typed command releases adaptive source-and-test development together.
    // Mirrors docs/compiler/control-plane.md#modes-and-releases.
    crate::control::release(&proj, &out, Some("generate"));
    let store = Store::load(&out);
    let result = crate::gen::run_all(&store, &runner, &gs, entities, opts.force, &trace);
    match &result {
        Ok(v) => trace.finish_transcript("done", v),
        Err(e) => trace.finish_transcript("failed", &serde_json::json!({"error": e})),
    }
    match result {
        Ok(sum) => {
            println!(
                "jazyk: gen done: {} generate goal(s) resolved, {} unchanged, {} failure(s)",
                sum["regenerated"], sum["skipped"], sum["failures"]
            );
            if sum["failures"].as_u64().unwrap_or(0) > 0 {
                1
            } else {
                0
            }
        }
        Err(e) => {
            eprintln!("jazyk: {}", e);
            if e.starts_with("unknown entity") {
                2
            } else {
                1
            }
        }
    }
}

// Parse a reply whose first line must be `FILE: <relative path>`; returns (path, body).
// Verification: run the ledger's tests and record verdicts. Programmatic rows execute
// their command; llm rows drive the configured model against the verify_task package.
// Mirrors docs/consumers/gen.md#runners.
pub fn run_test(opts: &Options, targets: &[String]) -> i32 {
    let (proj, llm, out) = resolve(&[], opts);
    if opts.verbose {
        llm::set_verbose(true);
    }
    let store = Store::load(&out);
    let gs = crate::gen::GenSettings::resolve(&proj);
    if opts.audit {
        let r = crate::verify::audit(&store, &gs);
        println!("jazyk: audit: {}", r);
        return 0;
    }
    if opts.list {
        // One row per requirement: id, statement, status.
        // Mirrors docs/frontends/cli.md#jazyk-test.
        let selected =
            crate::verify::select_rows(&store, &gs, targets, opts.kind.as_deref(), opts.force);
        for r in &selected {
            let rid = r["requirement"].as_str().unwrap_or("");
            let statement = store
                .graph
                .requirements
                .get(store.resolve_id(rid))
                .map(|q| q.statement.clone())
                .unwrap_or_default();
            // A row that is not verified says why and how to repair it.
            // Mirrors docs/frontends/cli.md#jazyk-test.
            let status = r["status"].as_str().unwrap_or("");
            let mut tail = String::new();
            if status != "verified" {
                if let Some(reason) = r["reason"].as_str().filter(|x| !x.is_empty()) {
                    tail.push_str(&format!("  {}", reason));
                }
                if let Some(repair) = r["repair"].as_str().filter(|x| !x.is_empty()) {
                    tail.push_str(&format!("  {}", repair));
                }
            }
            println!(
                "{:24} {:60} {}{}",
                rid,
                llm::truncate(&statement, 58),
                status,
                tail
            );
        }
        println!("jazyk: {} verify goal(s)", selected.len());
        return 0;
    }
    // Render the worker events on the historical CLI output format.
    use crate::session::TraceEvent;
    let sink: std::sync::Arc<dyn Fn(&TraceEvent) + Send + Sync> =
        std::sync::Arc::new(|ev| match ev {
            TraceEvent::VerifyRowDone {
                requirement,
                verdict,
                run,
                ..
            } => println!(
                "jazyk: {} {} ({})",
                requirement,
                if verdict == "pass" {
                    "verified"
                } else {
                    "FAILING"
                },
                run
            ),
            TraceEvent::VerifyRowStale {
                requirement,
                entity,
                status,
                reason,
            } => eprintln!(
                "jazyk: {} is {} ({}); generate with `jazyk gen {}`",
                requirement, status, reason, entity
            ),
            TraceEvent::VerifyRowError {
                requirement,
                message,
            } => eprintln!("jazyk: {}{}", requirement, message),
            _ => {}
        });
    let trace = Trace::to_sink(TraceLevel::Normal, sink, Default::default())
        .with_transcript(&out, "verify");
    let runner = match runner_for(&proj, &llm, &out) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("jazyk: {}", e);
            return 2;
        }
    };
    let result = crate::verify::run_all(
        &store,
        &runner,
        &gs,
        targets,
        opts.kind.as_deref(),
        opts.force,
        &trace,
    );
    match &result {
        Ok(v) => trace.finish_transcript("done", v),
        Err(e) => trace.finish_transcript("failed", &serde_json::json!({"error": e})),
    }
    match result {
        Ok(sum) => {
            let (verified, failing, stale, skipped, runner_failed) = (
                sum["verified"].as_u64().unwrap_or(0),
                sum["failing"].as_u64().unwrap_or(0),
                sum["stale"].as_u64().unwrap_or(0),
                sum["skipped"].as_u64().unwrap_or(0),
                sum["runnerFailed"].as_u64().unwrap_or(0),
            );
            if sum["rows"].as_u64().unwrap_or(0) == 0 {
                println!("jazyk: nothing to do; every targeted verify goal is verified");
            } else {
                println!(
                    "jazyk: test done: {} verify goal(s); {} verified, {} failing, {} stale, {} skipped{}",
                    sum["rows"],
                    verified,
                    failing,
                    stale,
                    skipped,
                    // A run the machine broke reads differently from a run the
                    // deliverable failed (docs/consumers/gen.md#runners).
                    if runner_failed > 0 {
                        format!(", {} not judged (the test runner failed)", runner_failed)
                    } else {
                        String::new()
                    }
                );
            }
            if failing > 0 || stale > 0 || skipped > 0 || runner_failed > 0 {
                1
            } else {
                0
            }
        }
        Err(e) => {
            eprintln!("jazyk: {}", e);
            1
        }
    }
}
