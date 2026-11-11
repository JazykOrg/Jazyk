// Build commands: `compile`, `check`, and `watch` with its helpers.
// Mirrors docs/frontends/cli.md#jazyk-compile.
use super::{resolve, set_claims, trace_for, verdict_line, Options};
use crate::llm;
use crate::project::{self, Project};
use crate::reconcile;
use crate::session::{Trace, TraceLevel};
use crate::store::Store;
use std::path::PathBuf;

pub fn run_compile(paths: &[String], opts: &Options) -> i32 {
    use crate::document_sets::Workspace;
    crate::logs::set_recording(opts.record);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = project::find_root(&cwd).unwrap_or_else(|| cwd.clone());
    let workspace = match Workspace::load(&root) {
        Ok(w) => w,
        Err((_, e)) => {
            eprintln!("jazyk: {}", e);
            return 2;
        }
    };
    if workspace.sets.is_empty() {
        eprintln!("jazyk: no document sets; use `jazyk sets add NAME --docs PATH`");
        return 2;
    }
    // Selected paths are relative to the working directory, like positional ones;
    // the selection holds them relative to the project root.
    let mut selection: Vec<crate::selection::Item> = Vec::new();
    for arg in &opts.select {
        match crate::selection::parse_arg(arg) {
            Ok(mut item) => {
                let abs = cwd.join(&item.path);
                let rel = abs.strip_prefix(&root).unwrap_or(&abs);
                item.path = rel.to_string_lossy().replace('\\', "/");
                selection.push(item);
            }
            Err(e) => {
                eprintln!("jazyk: --select {}", e);
                return 2;
            }
        }
    }
    let selection = (!selection.is_empty()).then(|| crate::selection::normalize(selection));
    let select_paths: Vec<String> = opts
        .select
        .iter()
        .filter_map(|a| crate::selection::parse_arg(a).ok().map(|i| i.path))
        .collect();
    let paths: Vec<String> = paths.iter().cloned().chain(select_paths).collect();
    let paths = &paths[..];
    // A path selects the sets claiming it; one no set claims selects nothing and is
    // a usage error, never a silent no-op (docs/frontends/cli.md#jazyk-compile).
    if opts.set.is_none() {
        if let Some(path) = paths.iter().find(|p| {
            !workspace
                .sets
                .iter()
                .any(|set| set_claims(&workspace, set, &cwd, p))
        }) {
            eprintln!("jazyk: no document set claims `{}`; `jazyk sets list` shows what each set compiles",path);
            return 2;
        }
    }
    let summary = workspace.summary();
    let mut code = 0;
    let mut selected = 0;
    for set in &workspace.sets {
        if let Some(selector) = opts.set.as_deref() {
            match workspace.select(selector) {
                Ok(s) if s.id == set.id => {}
                Ok(_) => continue,
                Err((_, e)) => {
                    eprintln!("jazyk: {}", e);
                    return 2;
                }
            }
        } else if !paths.is_empty() {
            if !paths
                .iter()
                .any(|path| set_claims(&workspace, set, &cwd, path))
            {
                continue;
            }
        } else if !summary["sets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == set.id && s["dirty"] == true && s["state"] != "empty")
        {
            continue;
        }
        selected += 1;
        let mut options = opts.clone();
        options.set = Some(set.id.clone());
        let (proj, llm, out) = resolve(&[], &options);
        if let Some(error) = proj.config_error.as_deref() {
            eprintln!("jazyk: {}", error);
            return 2;
        }
        // A set holding a newer store is refused; the other sets still build
        // (docs/compiler/graph.md#store-version).
        if let Some(why) = crate::store::newer_store(&out) {
            eprintln!("jazyk: {}: {}", set.name, why);
            code = 2;
            continue;
        }
        println!("{}", set.name);
        if opts.verbose {
            llm::set_verbose(true);
        }
        if let Some(items) = &selection {
            if let Err(e) = crate::selection::validate(&proj, items) {
                eprintln!("jazyk: --select {}", e);
                return 2;
            }
        }
        let _selection = selection
            .clone()
            .map(|items| crate::selection::enter(crate::selection::Handle::new(Some(items))));
        let trace = trace_for(opts).with_transcript(&out, "compile");
        let report = reconcile::compile_with(&proj, &llm, &out, &trace, opts.sessions);
        trace.finish_transcript("done", &serde_json::json!(report));
        println!("{}: {}", set.name, verdict_line(&report));
        if !report.converged() {
            code = 1;
        }
    }
    if selected == 0 {
        println!("No dirty document sets.");
    }
    code
}

pub fn run_check(paths: &[String], opts: &Options) -> i32 {
    let (proj, llm, out) = resolve(paths, opts);
    let trace = trace_for(opts).with_transcript(&out, "compile");
    let report = reconcile::compile(&proj, &llm, &out, &trace);
    trace.finish_transcript("done", &serde_json::json!(report));
    println!("{}", verdict_line(&report));
    let store = Store::load(&out);
    let mut errors = 0;
    for d in store.graph.diagnostics.values() {
        if d.lifecycle == "open"
            && d.severity == "error"
            && d.triage.as_deref() != Some("suppressed")
        {
            errors += 1;
            eprintln!(
                "error[{}]: {} ({})",
                d.rule,
                d.message,
                d.subjects.join(", ")
            );
        }
    }
    if errors > 0 || !report.converged() {
        1
    } else {
        0
    }
}

// One watch-loop build: goal lines instead of the tool-row trace, unless --verbose
// asked for the whole thing. Mirrors docs/frontends/cli.md#jazyk-watch.
fn watch_build(paths: &[String], opts: &Options) -> i32 {
    if opts.verbose {
        return run_compile(paths, opts);
    }
    if opts.set.is_none() {
        return run_compile(paths, opts);
    }
    let (proj, llm, out) = resolve(paths, opts);
    let level = if opts.quiet {
        TraceLevel::Quiet
    } else {
        TraceLevel::Normal
    };
    let sink: std::sync::Arc<dyn Fn(&crate::session::TraceEvent) + Send + Sync> =
        std::sync::Arc::new(goal_line);
    let trace = Trace::to_sink(level, sink, Default::default()).with_transcript(&out, "compile");
    let report = reconcile::compile(&proj, &llm, &out, &trace);
    trace.finish_transcript("done", &serde_json::json!(report));
    println!("{}", verdict_line(&report));
    if report.converged() {
        0
    } else {
        1
    }
}

// One line per goal event, the watch rendering: what opened (with its cause), what
// session took it, and how it ended (resolved with its justification, failed with
// its reason, parked). Board summaries and gc bursts print as in compile.
// Mirrors docs/frontends/cli.md#jazyk-watch.
fn goal_line(ev: &crate::session::TraceEvent) {
    use crate::session::TraceEvent;
    match ev {
        TraceEvent::Board {
            goals,
            kinds,
            blocked,
            ..
        } => {
            let per_kind: Vec<String> = kinds.iter().map(|(k, n)| format!("{} {}", n, k)).collect();
            let mut s = format!("compile: {} goals", goals);
            if !per_kind.is_empty() {
                s.push_str(&format!(" ({})", per_kind.join(", ")));
            }
            if *blocked > 0 {
                s.push_str(&format!(", {} blocked", blocked));
            }
            eprintln!("{}", s);
        }
        TraceEvent::GcBurst {
            goal_kind,
            target,
            count,
            limit,
            ..
        } => eprintln!("gc burst: {} {} ({} > {})", goal_kind, target, count, limit),
        TraceEvent::SessionStart { label, goals, .. } => {
            for g in goals {
                eprintln!("taken    {}  {}", g, label);
            }
        }
        TraceEvent::Goal {
            goal,
            event,
            cause,
            justification,
            reason,
            ..
        } => {
            let tail = match (cause, justification, reason) {
                (Some(c), _, _) => format!("  (g{} via {})", c.generation, c.via),
                (_, Some(j), _) => format!("  {}", j),
                (_, _, Some(r)) => format!("  {}", r),
                _ => String::new(),
            };
            eprintln!("{:<8} {}{}", event, goal, tail);
        }
        TraceEvent::SessionFailed { label, error, .. } => {
            eprintln!("[{}] session failed: {}", label, error)
        }
        _ => {}
    }
}

pub fn run_watch(paths: &[String], opts: &Options) -> i32 {
    let (proj, _llm, out) = resolve(paths, opts);
    let fingerprint = |proj: &Project| -> String {
        let Ok(w) = crate::document_sets::Workspace::load(&proj.root) else {
            return String::new();
        };
        let mut s = w.hash.clone();
        for f in w
            .files()
            .iter()
            .filter(|f| !w.memberships(f).is_empty())
            .map(|f| w.root.join(f))
        {
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
    // Native file events via the notify crate; the fingerprint over the matched doc
    // files decides whether a build actually runs, so editor temp files, renames, and
    // the out directory's own writes never trigger one.
    use notify::Watcher;
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let mut watcher =
        match notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if res.is_ok() {
                tx.send(()).ok();
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                eprintln!(
                    "jazyk: file watcher unavailable ({}); falling back to polling",
                    e
                );
                let mut last = String::new();
                loop {
                    let fp = fingerprint(&proj);
                    if fp != last {
                        last = fp;
                        watch_build(paths, opts);
                    }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
        };
    if let Err(e) = watcher.watch(&proj.root, notify::RecursiveMode::Recursive) {
        eprintln!("jazyk: cannot watch {}: {}", proj.root.display(), e);
        return 1;
    }
    println!("jazyk: watching {} (Ctrl-C to stop)", proj.root.display());
    // An incomplete build (work parked, e.g. by a transient endpoint outage) retries on
    // its own with backoff instead of idling until the next edit. Unfinished work is
    // never silent, and watch is the loop that owns resuming it. Mirrors
    // docs/frontends/cli.md#jazyk-watch.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut backoff = std::time::Duration::from_secs(30);
    let max_backoff = std::time::Duration::from_secs(300);
    let mut last = fingerprint(&proj);
    watch_build(paths, opts);
    loop {
        let retry_due = if watch_incomplete(&watch_stores(&proj.root, &cwd, paths, opts, &out)) {
            eprintln!(
                "jazyk: build incomplete; retrying parked work in {}s",
                backoff.as_secs()
            );
            match rx.recv_timeout(backoff) {
                Ok(()) => false,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => true,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            backoff = std::time::Duration::from_secs(30);
            if rx.recv().is_err() {
                break;
            }
            false
        };
        if retry_due {
            backoff = (backoff * 2).min(max_backoff);
            watch_build(paths, opts);
            continue;
        }
        // Debounce: editors save in bursts; let the burst finish.
        std::thread::sleep(std::time::Duration::from_millis(300));
        while rx.try_recv().is_ok() {}
        let fp = fingerprint(&proj);
        if fp != last {
            last = fp;
            backoff = std::time::Duration::from_secs(30);
            watch_build(paths, opts);
        }
    }
    0
}

// The stores whose verdict decides a watch retry: the named set's with `--set`,
// otherwise each set the watch's builds compile (the sets the paths claim, or every
// set). Mirrors docs/frontends/cli.md#jazyk-watch.
pub(super) fn watch_stores(
    root: &std::path::Path,
    cwd: &std::path::Path,
    paths: &[String],
    opts: &Options,
    named: &std::path::Path,
) -> Vec<PathBuf> {
    if opts.set.is_some() {
        return vec![named.to_path_buf()];
    }
    let Ok(w) = crate::document_sets::Workspace::load(root) else {
        return Vec::new();
    };
    w.sets
        .iter()
        .filter(|set| paths.is_empty() || paths.iter().any(|p| set_claims(&w, set, cwd, p)))
        .filter_map(|set| w.resolve(Some(&set.id)).ok().map(|p| p.out))
        .collect()
}

pub(super) fn watch_incomplete(stores: &[PathBuf]) -> bool {
    stores
        .iter()
        .any(|out| Store::load(out).status.verdict.state == "incomplete")
}
