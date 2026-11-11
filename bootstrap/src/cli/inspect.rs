// Inspection commands: `status`, `preview`, `explain`, `ripple`, `context`, `viewer`,
// and `query`. Mirrors docs/frontends/cli.md#jazyk-status.
use super::{resolve, run_sets, Options};
use crate::context;
use crate::project::{self, Project};
use crate::reconcile;
use crate::store::Store;
use std::path::PathBuf;

// status.yaml and the board: the store version, the last verdict with its counts,
// the live board counts, coverage, diagnostics, the last build's cost, and the
// unclaimed report. Mirrors docs/frontends/cli.md#jazyk-status.
pub fn run_status(opts: &Options) -> i32 {
    // No set: the aggregate read over every set, as text unless JSON is asked for.
    // Mirrors docs/frontends/cli.md#jazyk-status.
    if opts.set.is_none() {
        if opts.json || opts.costs {
            return run_sets(&["list".into()], opts);
        }
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let root = project::find_root(&cwd).unwrap_or(cwd);
        return match crate::document_sets::Workspace::load(&root) {
            Ok(workspace) => {
                print!("{}", render_sets_overview(&workspace.summary()));
                // The last line names where this project's logs live
                // (docs/frontends/cli.md#jazyk-status).
                println!("logs: {}", crate::layout::log_dir_for_root(&root).display());
                0
            }
            Err((_, e)) => {
                eprintln!("jazyk: {}", e);
                2
            }
        };
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (proj, _llm, out) = resolve(&[], opts);
    // Outside a project the store is the ad hoc one at the working directory: say
    // so, because every other line would otherwise read as a project's.
    if project::find_root(&cwd).is_none() {
        eprintln!(
            "jazyk: no jazyk.toml above {}; reading the ad hoc store at {} (`jazyk init` starts a project here)",
            cwd.display(),
            out.display()
        );
    }
    if opts.costs {
        let report = crate::costs::report_at(&out);
        if opts.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).unwrap_or_default()
            );
        } else {
            print!("{}", crate::costs::render(&report));
        }
        return 0;
    }
    let store = Store::load(&out);
    // The board is derived from disk the same way compile derives it, so on a tree
    // with pending edits this shows the goals the next build will run.
    let board = crate::board::Board::compute(&proj, &out);
    let report = status_report(&proj, &store, &board);
    if opts.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    } else {
        print!("{}", render_status(&report));
        println!("logs: {}", crate::layout::project_log_dir(&out).display());
    }
    0
}

// The aggregate status: one line per document set from the sets summary, then its
// selection warnings. Mirrors docs/frontends/cli.md#jazyk-status.
pub(crate) fn render_sets_overview(summary: &serde_json::Value) -> String {
    let sets = summary["sets"].as_array().cloned().unwrap_or_default();
    if sets.is_empty() {
        return "no document sets yet; `jazyk sets add NAME --docs GLOB` creates one\n".into();
    }
    let mut out = String::new();
    for set in &sets {
        let n = |k: &str| set["counts"][k].as_u64().unwrap_or(0);
        let verdict = set["verdict"]["state"]
            .as_str()
            .filter(|v| !v.is_empty())
            .unwrap_or("none");
        out.push_str(&format!(
            "{}: {}, generation {}, verdict {}; {} open, {} ready, {} blocked, {} parked, {} failed\n",
            set["name"].as_str().unwrap_or("?"),
            set["state"].as_str().unwrap_or("?"),
            set["generation"].as_u64().unwrap_or(0),
            verdict,
            n("open"),
            n("ready"),
            n("blocked"),
            n("parked"),
            n("failed"),
        ));
        for w in set["warnings"].as_array().into_iter().flatten() {
            out.push_str(&format!("  warning: {}\n", w.as_str().unwrap_or("")));
        }
    }
    out
}

// Everything `jazyk status` prints, gathered once: the text and the JSON render the
// same object. Mirrors docs/frontends/cli.md#jazyk-status.
pub(crate) fn status_report(
    proj: &Project,
    store: &Store,
    board: &crate::board::Board,
) -> serde_json::Value {
    use serde_json::json;
    let s = &store.status;
    let counts = board.counts();
    let open_of = |class: &str| {
        board
            .goals
            .iter()
            .filter(|g| {
                matches!(
                    g.state,
                    crate::model::GoalState::Open | crate::model::GoalState::Parked
                ) && g.mandatory
                    && g.class == class
            })
            .count()
    };
    let (mut total, mut covered) = (0usize, 0usize);
    for rec in store.docs.values() {
        for (r, sec) in &rec.sections {
            if !crate::goals::section_has_body(sec) {
                continue;
            }
            total += 1;
            if rec.coverage.contains_key(r) {
                covered += 1;
            }
        }
    }
    let pct = (covered * 100).checked_div(total).unwrap_or(100);
    let mut by_sev: std::collections::BTreeMap<&str, usize> = Default::default();
    for d in store.graph.diagnostics.values() {
        if d.lifecycle == "open" && d.triage.as_deref() != Some("suppressed") {
            *by_sev.entry(d.severity.as_str()).or_default() += 1;
        }
    }
    let shape = reconcile::shape(store);
    // The last build's cost, with the biggest goal kind's share beside the totals.
    let c = &s.costs;
    let cost = if c.sessions > 0 || c.tokens > 0 {
        let top = c
            .by_kind
            .iter()
            .max_by_key(|(_, l)| (l.tokens, l.sessions))
            .map(|(kind, l)| {
                let share = (l.tokens * 100)
                    .checked_div(c.tokens)
                    .unwrap_or_else(|| l.sessions * 100 / c.sessions.max(1));
                json!({"kind": kind, "share": share})
            });
        json!({"sessions": c.sessions, "tokens": c.tokens, "top": top})
    } else {
        serde_json::Value::Null
    };
    // The unattached remainder, summed over the ledger: generated mass no
    // requirement claims. Mirrors docs/consumers/gen.md#the-unattached-remainder.
    let ledger = crate::gen::Ledger::load(&store.out);
    let measured: Vec<(&String, &crate::gen::Unattached)> = ledger
        .entities
        .iter()
        .filter_map(|(slug, e)| e.unattached.as_ref().map(|u| (slug, u)))
        .collect();
    let (ufiles, ulines) = measured
        .iter()
        .fold((0u64, 0u64), |(f, l), (_, u)| (f + u.files, l + u.lines));
    let unattached = if ufiles > 0 || ulines > 0 {
        let worst = measured
            .iter()
            .max_by(|a, b| {
                a.1.ratio
                    .partial_cmp(&b.1.ratio)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap();
        json!({"files": ufiles, "lines": ulines, "worst": {"entity": worst.0, "ratio": worst.1.ratio}})
    } else {
        serde_json::Value::Null
    };
    // The unclaimed report: deliverable files no binding names, the decompilation
    // worklist. Mirrors docs/consumers/bind.md#the-unclaimed-report.
    let gs = crate::gen::GenSettings::resolve(proj);
    let unclaimed = crate::bind::unclaimed(proj, store, &gs);
    // The medium warning: the ledger's toolchain and the recorded run commands
    // disagree. Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
    let medium = crate::gen::medium_divergence(&ledger);
    // What moves the board from here, one line per owed action, the human's first.
    let human: Vec<&crate::model::Goal> = board
        .goals
        .iter()
        .filter(|g| {
            matches!(g.state, crate::model::GoalState::Blocked { .. })
                && crate::goals::blocked_on_human(&g.kind)
        })
        .collect();
    let failed: Vec<&crate::model::Goal> = board
        .goals
        .iter()
        .filter(|g| matches!(g.state, crate::model::GoalState::Failed { .. }))
        .collect();
    let mut next: Vec<String> = Vec::new();
    // No verdict on record: no build has run here yet.
    if s.verdict.state.is_empty() {
        next.push("`jazyk compile` runs the first build".to_string());
    }
    if let Some(g) = human.first() {
        next.push(format!(
            "{} goal(s) wait on a human: `jazyk answer {}` lists the options",
            human.len(),
            g.id
        ));
    }
    if counts.ready > 0 {
        let ledger_ready = board.ready_of(&crate::board::LEDGER_KINDS);
        let compile_ready = counts.ready.saturating_sub(ledger_ready);
        if compile_ready > 0 {
            next.push(format!(
                "{} goal(s) ready: `jazyk compile` runs them (`jazyk preview` shows the first prompt)",
                compile_ready
            ));
        }
        if ledger_ready > 0 {
            next.push(format!(
                "{} bind, generate, or verify goal(s) ready: `jazyk gen` binds and generates, `jazyk test` verifies",
                ledger_ready
            ));
        }
    }
    if counts.gated > 0 {
        let ledger_gated = board.gated_of(&crate::board::LEDGER_KINDS);
        let compile_gated = counts.gated.saturating_sub(ledger_gated);
        if compile_gated > 0 {
            next.push(format!(
                "{} goal(s) await a compile release: `jazyk release compile` approves them (or `jazyk compile`, a typed command is an approval)",
                compile_gated
            ));
        }
        if ledger_gated > 0 {
            next.push(format!(
                "{} bind or generate goal(s) await a generate release: `jazyk release generate` approves them (or `jazyk gen`)",
                ledger_gated
            ));
        }
    }
    if let Some(g) = failed.first() {
        next.push(format!(
            "{} failed goal(s): `jazyk explain {}` shows the reason",
            failed.len(),
            g.id
        ));
    }
    if next.is_empty() {
        if board.open_goals().is_empty() {
            next.push("nothing to do; the graph reflects the docs".to_string());
        } else {
            next.push(
                "no goal is ready; `jazyk explain` says what each open goal waits for".to_string(),
            );
        }
    }
    json!({
        "root": proj.root.display().to_string(),
        "out": store.out.display().to_string(),
        "version": s.version,
        "generation": s.generation,
        "verdict": s.verdict.to_string(),
        "board": {
            "open": counts.open,
            "compile": open_of("compile"),
            "gc": open_of("gc"),
            "blocked": counts.blocked,
            "parked": counts.parked,
            "failed": counts.failed,
            "optional": counts.optional,
            "ready": counts.ready,
            "gated": counts.gated,
            "waiting": board.waiting.len(),
        },
        "coverage": {"percent": pct, "covered": covered, "total": total},
        "diagnostics": by_sev,
        "proposals": store.pending_proposals(),
        "medium": medium,
        "shape": {"perDepth": shape.per_depth, "bands": shape.bands, "line": shape_line(store)},
        "cost": cost,
        "unattached": unattached,
        "unclaimed": unclaimed,
        "next": next,
    })
}

// The text form of the status report, one line per field in the documented order.
pub(crate) fn render_status(r: &serde_json::Value) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "store: version {}, generation {}\n",
        r["version"], r["generation"]
    ));
    s.push_str(&format!(
        "verdict: {}\n",
        r["verdict"].as_str().unwrap_or("")
    ));
    let b = &r["board"];
    s.push_str(&format!(
        "board: {} open ({} compile, {} gc), {} blocked, {} parked, {} failed, {} optional{}\n",
        b["open"],
        b["compile"],
        b["gc"],
        b["blocked"],
        b["parked"],
        b["failed"],
        b["optional"],
        match b["waiting"].as_u64().unwrap_or(0) {
            0 => String::new(),
            n => format!(", {} pair(s) waiting under the cap", n),
        }
    ));
    let c = &r["coverage"];
    s.push_str(&format!(
        "coverage: {}% ({} of {} sections)\n",
        c["percent"], c["covered"], c["total"]
    ));
    let diags = r["diagnostics"].as_object();
    let proposals = r["proposals"].as_u64().unwrap_or(0);
    s.push_str(&format!(
        "diagnostics: {}\n",
        match diags {
            Some(m) if !m.is_empty() => m
                .iter()
                .map(|(k, v)| {
                    let n = v.as_u64().unwrap_or(0);
                    format!("{} {}{}", n, k, if n == 1 { "" } else { "s" })
                })
                .collect::<Vec<_>>()
                .join(", "),
            _ => "none".to_string(),
        }
    ));
    if proposals > 0 {
        s.push_str(&format!(
            "proposals: {} await ratification (`jazyk answer` lists them)\n",
            proposals
        ));
    }
    if let Some(m) = r["medium"].as_str() {
        s.push_str(&format!("medium: {}\n", m));
    }
    s.push_str(&format!("{}\n", r["shape"]["line"].as_str().unwrap_or("")));
    if let Some(cost) = r["cost"].as_object() {
        let tokens = cost["tokens"].as_u64().unwrap_or(0);
        let tokens = if tokens >= 1000 {
            format!("{}k", tokens / 1000)
        } else {
            tokens.to_string()
        };
        let mut line = format!("cost: {} sessions, {} tokens", cost["sessions"], tokens);
        if let Some(top) = cost["top"].as_object() {
            if top["share"].as_u64().unwrap_or(0) > 0 {
                line.push_str(&format!(
                    " ({}% {})",
                    top["share"],
                    top["kind"].as_str().unwrap_or("")
                ));
            }
        }
        s.push_str(&line);
        s.push('\n');
    }
    if let Some(u) = r["unattached"].as_object() {
        s.push_str(&format!(
            "unattached: {} file(s), {} line(s) (worst {} at {:.2})\n",
            u["files"],
            u["lines"],
            u["worst"]["entity"].as_str().unwrap_or(""),
            u["worst"]["ratio"].as_f64().unwrap_or(0.0)
        ));
    }
    let un: Vec<&str> = r["unclaimed"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if !un.is_empty() {
        s.push_str(&format!(
            "unclaimed: {} file(s) no binding names (`jazyk decompile` drafts docs for them)\n",
            un.len()
        ));
        for f in un.iter().take(8) {
            s.push_str(&format!("  - {}\n", f));
        }
        if un.len() > 8 {
            s.push_str(&format!("  ... and {} more\n", un.len() - 8));
        }
    }
    if let Some(next) = r["next"].as_array() {
        for n in next.iter().filter_map(|v| v.as_str()) {
            s.push_str(&format!("next: {}\n", n));
        }
    }
    s
}

// The shape line: the entity count per depth of the containment tree (the parentless
// entities at depth 1), then the fan-out histogram, how many levels hold how many
// direct children, banded against the `children-per-entity` registry values: at or
// under soft, over soft and at or under hard, over hard. An empty tree prints `0`.
// Mirrors docs/frontends/cli.md#jazyk-status.
pub fn shape_line(store: &Store) -> String {
    let shape = reconcile::shape(store);
    let depths = if shape.per_depth.is_empty() {
        "0".to_string()
    } else {
        shape
            .per_depth
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" / ")
    };
    let (soft, hard) = crate::limits::limit(crate::limits::CHILDREN_PER_ENTITY)
        .map(|l| (l.soft, l.hard))
        .unwrap_or((0, 0));
    format!(
        "shape: {} nodes per depth; fan-out 2-{}: {}, {}-{}: {}, over {}: {}",
        depths,
        soft,
        shape.bands[0],
        soft + 1,
        hard,
        shape.bands[1],
        hard,
        shape.bands[2]
    )
}

// The next session's prompt, exactly as the model would receive it. With a goal or a
// target, the batch that goal would join; a goal that is not ready renders behind a
// `not ready:` line; blocked-on-human kinds print what the human owes instead.
// Mirrors docs/frontends/cli.md#jazyk-preview.
pub fn run_preview(args: &[String], opts: &Options) -> i32 {
    let (proj, _llm, out) = resolve(&[], opts);
    let (store, control, board) = board_from_disk(&proj, &out);
    let target = args.first().map(String::as_str).unwrap_or("").trim();
    match preview_text(&store, &control, &board, target, &proj.linting) {
        Ok(text) => {
            print!("{}", text);
            0
        }
        Err(text) => {
            // What the human owes prints on stdout (it is the answer); a reason
            // nothing rendered prints on stderr.
            if text.starts_with("jazyk:") {
                eprintln!("{}", text);
            } else {
                print!("{}", text);
            }
            1
        }
    }
}

// The store with the documents on disk synced in, the control plane, and the board
// derived from them: what `preview` and `explain` read. Nothing is committed.
pub(super) fn board_from_disk(
    proj: &Project,
    out: &std::path::Path,
) -> (Store, crate::control::Control, crate::board::Board) {
    let mut store = Store::load(out);
    let (parsed, _) = reconcile::parse_all(proj);
    store.sync_docs(&parsed);
    let control = crate::control::Control::load(proj, out);
    let board = crate::board::Board::derive(&store, proj, &control);
    (store, control, board)
}

// The prompt a batch receives, assembled the way the runner, the MCP serving, and
// the GUI assemble it: the project block carries the mode the control plane holds
// and the batch id the board minted, so the protocol line names the batch
// `begin_goals` will accept. Mirrors docs/frontends/cli.md#jazyk-preview.
fn batch_prompt(
    store: &Store,
    control: &crate::control::Control,
    goals: &[crate::model::Goal],
    batch_id: &str,
    lint: &crate::project::Linting,
) -> String {
    let (loaded, skills) = crate::session::initial_loaded(store, goals);
    let mut pb =
        crate::session::ProjectBlock::compute(store, goals, &control.compile).with_lint(lint);
    pb.batch = batch_id.to_string();
    crate::session::session_prompt(store, goals, &loaded, &skills, &pb)
}

// The text `jazyk preview` prints: Ok with a rendered prompt (exit 0), Err with the
// human path or the reason nothing rendered (exit 1).
pub(crate) fn preview_text(
    store: &Store,
    control: &crate::control::Control,
    board: &crate::board::Board,
    target: &str,
    lint: &crate::project::Linting,
) -> Result<String, String> {
    if target.is_empty() {
        // The batch the scheduler would claim next.
        let Some(batch) = board.batches.first() else {
            return Err(if board.open_goals().is_empty() {
                format!(
                    "jazyk: nothing to preview; the board is empty ({})",
                    board.verdict()
                )
            } else {
                "jazyk: no ready batch; `jazyk explain` says what every open goal waits for"
                    .to_string()
            });
        };
        return Ok(batch_prompt(
            store,
            control,
            &batch_goals(board, batch),
            &batch.id,
            lint,
        ));
    }
    // A batch id, as the board lists them.
    if let Some(batch) = board.batches.iter().find(|b| b.id == target) {
        return Ok(batch_prompt(
            store,
            control,
            &batch_goals(board, batch),
            &batch.id,
            lint,
        ));
    }
    // A goal id, or a target with goals on it: a node id, a section reference, or a
    // document path (whose goals sit on its sections).
    let goal = board.goal(target).cloned().or_else(|| {
        let doc_prefix = (!target.contains('#')).then(|| format!("{}#", target));
        let on_target: Vec<&crate::model::Goal> = board
            .goals
            .iter()
            .filter(|g| {
                g.target == target
                    || doc_prefix
                        .as_ref()
                        .is_some_and(|p| g.target.starts_with(p.as_str()))
            })
            .collect();
        on_target
            .iter()
            .find(|g| board.is_ready(&g.id))
            .or_else(|| on_target.first())
            .map(|g| (*g).clone())
    });
    let Some(goal) = goal else {
        // A pair the degree cap holds back has no session yet; say so instead of
        // a prompt. Mirrors docs/frontends/cli.md#jazyk-explain.
        if let Some(w) = board.waiting_pair(target) {
            return Err(format!(
                "jazyk: g:rejudge-pair:{} waits under the cap (score {}; {} hold(s) {} open pairs); no session runs for it until a slot frees",
                w.target,
                w.score,
                w.full.join(" and "),
                crate::limits::PAIRS_PER_REQUIREMENT_CAP
            ));
        }
        return Err(format!(
            "jazyk: no goal on `{}`; `jazyk explain` lists the board, `jazyk explain {}` what a change there would open",
            target, target
        ));
    };
    // ratify and answer have no session: print the human path instead of a prompt.
    if crate::goals::blocked_on_human(&goal.kind) {
        let mut s = format!(
            "{} is blocked on a human; no session runs for it\n",
            goal.id
        );
        s.push_str(&format!(
            "  owed: {}\n",
            crate::session::gate_line(&goal.kind)
        ));
        s.push_str(&format!(
            "  change: {}\n",
            crate::session::change_line(&goal)
        ));
        for h in &goal.hints {
            s.push_str(&format!("  hint: {}\n", h));
        }
        s.push_str(&format!(
            "  next: `jazyk answer {}` lists the options\n",
            goal.id
        ));
        return Err(s);
    }
    // The batch the goal joins; a goal in no scheduled batch renders alone, behind
    // the readiness reason, under a placeholder id the scheduler has not minted.
    if let Some(batch) = board
        .batches
        .iter()
        .find(|b| b.goals.iter().any(|id| id == &goal.id))
    {
        return Ok(batch_prompt(
            store,
            control,
            &batch_goals(board, batch),
            &batch.id,
            lint,
        ));
    }
    let reason = match board.readiness.get(&goal.id) {
        Some(crate::goals::Ready::Blocked(r)) => r.clone(),
        _ if board.gated.contains(&goal.id) => {
            "gated, awaiting release (`jazyk release`, or the GUI)".to_string()
        }
        _ => match board.claimed.get(&goal.id) {
            Some(w) => format!("claimed by {}", w),
            None => "not scheduled in a ready batch".to_string(),
        },
    };
    let placeholder = format!("b{}-?", store.status.generation);
    Ok(format!(
        "not ready: {} (the batch id {} below is a placeholder until the scheduler forms its batch)\n{}",
        reason,
        placeholder,
        batch_prompt(
            store,
            control,
            std::slice::from_ref(&goal),
            &placeholder,
            lint
        )
    ))
}

fn batch_goals(
    board: &crate::board::Board,
    batch: &crate::board::Batch,
) -> Vec<crate::model::Goal> {
    batch
        .goals
        .iter()
        .filter_map(|id| board.goal(id))
        .cloned()
        .collect()
}

// Why a goal exists, what a change to a target would open, or the whole board.
// Mirrors docs/frontends/cli.md#jazyk-explain.
pub fn run_explain(args: &[String], opts: &Options) -> i32 {
    let (proj, _llm, out) = resolve(&[], opts);
    let (store, _control, board) = board_from_disk(&proj, &out);
    let target = args.first().map(String::as_str).unwrap_or("").trim();
    if target.is_empty() {
        println!("{}", board.summary_line());
        let rendered = board.render();
        if rendered.is_empty() {
            println!("verdict: {}", board.verdict());
        } else {
            println!("{}", rendered);
        }
        return 0;
    }
    match board.explain(&store, target) {
        Some(s) => {
            print!("{}", s);
            0
        }
        None => {
            eprintln!(
                "jazyk: `{}` names no goal and no known target; a goal is g:<kind>:<target>, a target is a node id (ent:..., req:..., view:...), a section reference (doc.md#/ref), or a document path",
                target
            );
            1
        }
    }
}

// The ripple DAG rooted at a change, rendered from the journal.
// Mirrors docs/frontends/cli.md#jazyk-ripple.
pub fn run_ripple(args: &[String], opts: &Options) -> i32 {
    let (_proj, _llm, out) = resolve(&[], opts);
    let store = Store::load(&out);
    match ripple_text_for(&store, args.first().map(String::as_str), opts.back) {
        Ok(text) => {
            print!("{}", text);
            0
        }
        Err(e) => {
            eprintln!("jazyk: {}", e);
            1
        }
    }
}

// The rendered ripple for a root; without one, the last build (the journal from just
// after the previous build's checks entry), so a bare `jazyk ripple` is the
// whole-build report.
pub(super) fn ripple_text_for(
    store: &Store,
    root: Option<&str>,
    back: bool,
) -> Result<String, String> {
    let root = match root {
        Some(r) => r.to_string(),
        None => {
            let entries = crate::goals::read_journal(&store.out);
            if entries.is_empty() {
                return Err("nothing to walk; the journal is empty".to_string());
            }
            let start = match entries.iter().rposition(|e| e.kind == "checks") {
                Some(i) => entries[..i]
                    .iter()
                    .rposition(|e| e.kind == "checks")
                    .map(|p| p + 1)
                    .unwrap_or(0),
                None => 0,
            };
            format!("g{}", entries[start].generation)
        }
    };
    match crate::reconcile::ripple(store, &root, back) {
        Some(tree) => Ok(crate::reconcile::render_ripple(&tree)),
        None => Err(format!(
            "nothing to walk from `{}`; the journal holds no entry touching it",
            root
        )),
    }
}

// `jazyk context <target>`: exactly what `load` renders, then the status block.
// `--expand HANDLE` follows the named handles first, as `expand` would.
// Mirrors docs/frontends/cli.md#jazyk-context.
pub fn run_context(opts: &Options, target: &str) -> i32 {
    let (_proj, _llm, out) = resolve(&[], opts);
    let store = Store::load(&out);
    let depth = opts.depth.unwrap_or(1);
    match context::cli_context(&store, target, depth, &opts.expand) {
        Ok(s) => {
            println!("{}", s);
            0
        }
        Err(e) => {
            eprintln!("jazyk: {}", e);
            1
        }
    }
}

// Render the graph into one self-contained HTML file. An `--out` value ending in .html
// names the file; anything else is the store's out directory as usual.
pub fn run_viewer(opts: &Options) -> i32 {
    let html_target = opts.out.clone().filter(|o| o.ends_with(".html"));
    let mut store_opts = opts.clone();
    if html_target.is_some() {
        store_opts.out = None;
    }
    let (proj, _llm, out) = resolve(&[], &store_opts);
    let store = Store::load(&out);
    let gs = crate::gen::GenSettings::resolve(&proj);
    let html = crate::viewer::render(&store, &gs);
    let path = html_target
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| out.join("graph.html"));
    if let Some(parent) = path.parent() {
        crate::vfs::create_dir_all(parent).ok();
    }
    match crate::vfs::write(&path, html) {
        Ok(()) => {
            println!("jazyk: wrote {}", path.display());
            0
        }
        Err(e) => {
            eprintln!("jazyk: write {}: {}", path.display(), e);
            1
        }
    }
}

// The search tool from the terminal: one `id (name): definition` line per match, or
// one JSON object per line under --json. Mirrors docs/frontends/cli.md#jazyk-query.
pub fn run_query(opts: &Options, query: &str) -> i32 {
    if opts.set.is_none() {
        let (project, _, _) = resolve(&[], opts);
        if let Ok(w) = crate::document_sets::Workspace::load(&project.root) {
            if w.sets.len() != 1 {
                let mut code = 0;
                for set in w.sets {
                    let mut local = opts.clone();
                    local.set = Some(set.id);
                    println!("{}", set.name);
                    code = code.max(run_query(&local, query));
                }
                return code;
            }
        }
    }
    let (_proj, _llm, out) = resolve(&[], opts);
    let store = Store::load(&out);
    let hits = store.search(query);
    if hits.is_empty() {
        if !opts.json {
            eprintln!("jazyk: no entity matches `{}`", query);
        }
        return 1;
    }
    for (id, name, def) in hits {
        if opts.json {
            println!(
                "{}",
                serde_json::json!({"id": id, "name": name, "definition": def})
            );
        } else {
            println!("{} ({}): {}", id, name, def);
        }
    }
    0
}
