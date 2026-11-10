// Control commands: `release`, `answer`, `monitor`, and `spend`.
// Mirrors docs/frontends/cli.md#jazyk-release.
use super::{resolve, Options};
use crate::project::Project;
use crate::store::Store;
use std::path::Path;

// Record a release: approve the pending changes for a stage (both when unnamed)
// without running anything. The scriptable form of the GUI's release button.
// Mirrors docs/frontends/cli.md#jazyk-release.
pub fn run_release(paths: &[String], opts: &Options) -> i32 {
    let stage = paths.first().map(String::as_str);
    if let Some(s) = stage {
        if s != "compile" && s != "generate" {
            eprintln!("jazyk: release takes `compile` or `generate` (or nothing for both)");
            return 2;
        }
    }
    let (proj, _llm, out) = resolve(&[], opts);
    let gated_before = crate::board::Board::compute(&proj, &out).counts().gated;
    crate::control::release(&proj, &out, stage);
    let board = crate::board::Board::compute(&proj, &out);
    let counts = board.counts();
    let approved = gated_before.saturating_sub(counts.gated);
    let ready_compile = board.ready_of(&crate::board::Board::graph_kinds());
    let ready_bind = board.ready_of(&["bind"]);
    let ready_generate = board.ready_of(&["generate"]);
    println!(
        "jazyk: released {}: {} gated goal(s) approved; ready now: {} compile, {} bind, {} generate",
        stage.unwrap_or("compile and generate"),
        approved,
        ready_compile,
        ready_bind,
        ready_generate,
    );
    if approved == 0 && gated_before == 0 {
        println!("jazyk: nothing was gated; in auto mode a release changes nothing");
    }
    if ready_compile > 0 {
        println!(
            "next: `jazyk compile` runs the compile goals (or an attached worker claims them)"
        );
    }
    if ready_bind + ready_generate > 0 {
        println!("next: `jazyk gen` runs the bind and generate goals");
    } else if counts.gated == 0 && ready_compile == 0 && !board.open_goals().is_empty() {
        println!("next: no goal is ready yet; `jazyk explain` says what each waits for");
    }
    0
}

// Answer one prompted diagnostic from the terminal: print the prompt with its options
// numbered, or hand the reply to the one answer engine every frontend calls. A
// `handling` reply runs its answer session in the foreground.
// Mirrors docs/frontends/cli.md#jazyk-answer.
pub fn run_answer(args: &[String], opts: &Options) -> i32 {
    let Some(target) = args.first() else {
        eprintln!("jazyk: `answer` takes a diagnostic id, a ratify goal, or an answer goal; `jazyk explain` lists the goals blocked on a human");
        return 2;
    };
    if opts.option.is_some() && opts.text.is_some() {
        eprintln!("jazyk: `answer` takes one reply: --option N or --text \"...\", not both");
        return 2;
    }
    let (proj, _llm, out) = resolve(&[], opts);
    let did = if let Some(rest) = target.strip_prefix("g:answer:") {
        rest.to_string()
    } else if target.starts_with("g:ratify:") {
        let board = crate::board::Board::compute(&proj, &out);
        match board
            .goal(target)
            .and_then(|g| g.change["proposal"].as_str().map(String::from))
        {
            Some(p) => p,
            None => {
                eprintln!("jazyk: `{}` names no ratify goal with a proposal", target);
                return 1;
            }
        }
    } else {
        target.clone()
    };
    let store = crate::store::Store::load(&out);
    let rid = store.resolve_id(&did).to_string();
    let Some(d) = store.graph.diagnostics.get(&rid) else {
        eprintln!("jazyk: unknown diagnostic `{}`", did);
        return 1;
    };
    let reply = match (opts.option, opts.text.as_ref()) {
        (Some(i), _) => crate::answer::Reply::Choice(i),
        (None, Some(t)) => crate::answer::Reply::Text(t.clone()),
        (None, None) => {
            println!("{}  {} on {}", rid, d.rule, d.subjects.join(", "));
            match &d.prompt {
                Some(p) => {
                    println!("  {}", p.question);
                    for (i, o) in p.options.iter().enumerate() {
                        match &o.edit {
                            Some(e) => println!(
                                "  {}  {} (edit {}#{}: {})",
                                i, o.label, e.doc, e.section, e.new_text
                            ),
                            None => println!("  {}  {} (answer)", i, o.label),
                        }
                    }
                    if p.freeform {
                        println!("  freeform accepted: --text \"...\"");
                    }
                }
                None => println!("  no prompt: nothing to choose"),
            }
            if let Some(a) = &d.answer {
                println!("  answered: {} ({})", a.text, a.status);
            }
            return 1;
        }
    };
    drop(store);
    match crate::answer::answer(&proj, &out, &rid, reply, None) {
        Ok(v) if v["status"] == "handling" => {
            println!("jazyk: recorded; an answer session acts on it");
            match crate::answer::run_handler(&proj, &out, &rid) {
                Ok(()) => {
                    println!("jazyk: handled");
                    0
                }
                Err(e) => {
                    eprintln!("jazyk: answer session failed: {}", e);
                    1
                }
            }
        }
        Ok(v) => {
            println!("jazyk: applied: {}", v["note"].as_str().unwrap_or_default());
            // The fact changes that rode with the prose, and the transitions left to
            // review. Mirrors docs/compiler/model/diagnostic.md#answers.
            for f in v["facts"].as_array().into_iter().flatten() {
                println!(
                    "  fact: {} {} = {}",
                    f["id"].as_str().unwrap_or_default(),
                    f["field"].as_str().unwrap_or_default(),
                    f["value"]
                );
            }
            for f in v["followed"].as_array().into_iter().flatten() {
                let t = &f["transition"];
                println!(
                    "  transition: {} {} -> {}",
                    f["id"].as_str().unwrap_or_default(),
                    t["from"].as_str().unwrap_or_default(),
                    t["to"].as_str().unwrap_or_default()
                );
            }
            for id in v["review"].as_array().into_iter().flatten() {
                println!(
                    "  transition left to review: {} (the rewrite does not map one to one)",
                    id.as_str().unwrap_or_default()
                );
            }
            0
        }
        Err(e) => {
            eprintln!("jazyk: {}", e);
            1
        }
    }
}

// The external agent's trigger: watch the same surfaces `watch` does, perform nothing,
// print the ready work and which MCP tool begins it on every state change. One block
// per notice, quiet otherwise; --json prints one object per line.
// Mirrors docs/frontends/cli.md#jazyk-monitor.
pub fn run_monitor(opts: &Options) -> i32 {
    let (proj, _llm, out) = resolve(&[], opts);
    let json_mode = opts.json;
    let gs = crate::gen::GenSettings::resolve(&proj);
    // The watched surfaces: docs plus the ledger and its files, same as await_changes.
    let fingerprint = |proj: &Project| -> String {
        let mut s = String::new();
        let mut files = proj.doc_files();
        files.extend(crate::gen::Ledger::files(&out));
        // A release or mode change is a state change: the notice fires on the click.
        files.push(crate::control::Control::path(&out));
        let ledger = crate::gen::Ledger::load(&out);
        for row in ledger.requirements.values() {
            for f in &row.files {
                files.push(gs.deliverable.join(f));
            }
        }
        files.sort();
        files.dedup();
        for f in files {
            if let Ok(md) = std::fs::metadata(&f) {
                s.push_str(&format!(
                    "{}:{}:{:?};",
                    f.display(),
                    md.len(),
                    md.modified().ok()
                ));
            }
        }
        s
    };
    // Prints the queue notice and reports whether actionable work exists (ready, not
    // gated, not claimed). Under --once the monitor stays silent until it does: one
    // notice, then exit 0. Gated work prints as awaiting release.
    let once = opts.once;
    let notice = |last: &mut String| -> bool {
        let board = crate::board::Board::compute(&proj, &out);
        let ready = board.ready_goals();
        let has_work = !ready.is_empty();
        if once && !has_work {
            return false;
        }
        // One JSON object per line: the object, then the newline a line reader waits
        // for. Mirrors docs/frontends/cli.md#jazyk-monitor.
        let rendered = if json_mode {
            format!("{}\n", board.answer())
        } else {
            let counts = board.counts();
            let mut s = String::new();
            if board.open_goals().is_empty() && counts.blocked == 0 {
                s.push_str(&format!("jazyk: nothing to do ({})\n", board.verdict));
            } else {
                s.push_str(&format!(
                    "jazyk: {} goals ready, {} blocked\n",
                    counts.ready, counts.blocked
                ));
                for g in ready.iter().take(5) {
                    s.push_str(&format!(
                        "  {} {} ({})\n",
                        g.kind,
                        g.target,
                        g.hints.first().cloned().unwrap_or_default()
                    ));
                }
                if ready.len() > 5 {
                    s.push_str(&format!("  ... and {} more ready\n", ready.len() - 5));
                }
                // What a human owes prints before what a release owes: the gated
                // goals share one line below, the human-blocked ones name their ask.
                let mut blocked: Vec<&crate::model::Goal> = board
                    .goals
                    .iter()
                    .filter(|g| matches!(g.state, crate::model::GoalState::Blocked { .. }))
                    .collect();
                blocked.sort_by_key(|g| !crate::goals::blocked_on_human(&g.kind));
                for g in blocked.iter().take(3) {
                    s.push_str(&format!(
                        "  blocked: {} {} ({})\n",
                        g.kind,
                        g.target,
                        board
                            .readiness
                            .get(&g.id)
                            .and_then(|r| r.reason())
                            .unwrap_or("")
                    ));
                }
                if blocked.len() > 3 {
                    s.push_str(&format!("  ... and {} more blocked\n", blocked.len() - 3));
                }
                if counts.gated > 0 {
                    s.push_str(&format!(
                        "  {} gated, awaiting release (`jazyk release` or the GUI)\n",
                        counts.gated
                    ));
                }
                if has_work {
                    let ledger_only = ready
                        .iter()
                        .all(|g| crate::board::LEDGER_KINDS.contains(&g.kind.as_str()));
                    s.push_str(if ledger_only {
                        "  → call binding_tasks, generation_tasks, or verification_tasks on the jazyk MCP server\n"
                    } else {
                        "  → call goals on the jazyk MCP server, then begin_goals to claim a batch\n"
                    });
                }
            }
            s
        };
        if rendered != *last {
            print!("{}", rendered);
            if !json_mode {
                println!();
            }
            use std::io::Write;
            std::io::stdout().flush().ok();
            *last = rendered;
        }
        has_work
    };
    let mut last_notice = String::new();
    if notice(&mut last_notice) && once {
        return 0;
    }
    use notify::Watcher;
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let mut watcher =
        match notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if res.is_ok() {
                tx.send(()).ok();
            }
        }) {
            Ok(w) => w,
            Err(_) => {
                // Polling fallback.
                let mut last_fp = fingerprint(&proj);
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    let fp = fingerprint(&proj);
                    if fp != last_fp {
                        last_fp = fp;
                        if notice(&mut last_notice) && once {
                            return 0;
                        }
                    }
                }
            }
        };
    if let Err(e) = watcher.watch(&proj.root, notify::RecursiveMode::Recursive) {
        eprintln!("jazyk: cannot watch {}: {}", proj.root.display(), e);
        return 1;
    }
    // The out directory can live outside the root (--out); watch it too so releases
    // fire events. Best effort: inside the root it is already covered.
    if !out.starts_with(&proj.root) {
        watcher
            .watch(&out, notify::RecursiveMode::NonRecursive)
            .ok();
    }
    let mut last_fp = fingerprint(&proj);
    loop {
        if rx.recv().is_err() {
            break;
        }
        // Debounce the burst, then re-check the fingerprint.
        std::thread::sleep(std::time::Duration::from_millis(300));
        while rx.try_recv().is_ok() {}
        let fp = fingerprint(&proj);
        if fp != last_fp {
            last_fp = fp;
            if notice(&mut last_notice) && once {
                return 0;
            }
        }
    }
    0
}

// `jazyk spend <generation> <tokens> [--cached N]`: an external agent's own report of
// its bill, stamped on a committed session entry that carries none.
// Mirrors docs/frontends/cli.md#jazyk-spend.
pub fn run_spend(args: &[String], opts: &Options) -> i32 {
    let (generation, tokens) = match (args.first(), args.get(1)) {
        (Some(g), Some(t)) => (g.as_str(), t.as_str()),
        _ => {
            eprintln!("jazyk spend: takes a generation (g42 or 42) and the tokens billed; see `jazyk spend --help`");
            return 2;
        }
    };
    let generation = match generation.trim_start_matches('g').parse::<u64>() {
        Ok(g) => g,
        Err(_) => {
            eprintln!(
                "jazyk spend: `{}` is not a generation (g42 or 42)",
                generation
            );
            return 2;
        }
    };
    let tokens = match tokens.parse::<u64>() {
        Ok(t) if t > 0 => t,
        _ => {
            eprintln!(
                "jazyk spend: tokens takes a whole number above zero, got `{}`",
                tokens
            );
            return 2;
        }
    };
    let (_proj, _llm, out) = resolve(&[], opts);
    match spend_at(&out, generation, tokens, opts.cached) {
        Ok(line) => {
            println!("{}", line);
            0
        }
        Err(e) => {
            eprintln!("jazyk spend: {}", e);
            1
        }
    }
}

// The stamp and its one-line report, over an out directory.
pub(super) fn spend_at(
    out: &Path,
    generation: u64,
    tokens: u64,
    cached: Option<u64>,
) -> Result<String, String> {
    let entry = Store::stamp_reported(out, generation, tokens, cached)?;
    Ok(format!(
        "g{}: {} tokens{}, reported by the agent",
        generation,
        tokens,
        entry
            .cached_tokens
            .map(|c| format!(", {} cached", c))
            .unwrap_or_default()
    ))
}
