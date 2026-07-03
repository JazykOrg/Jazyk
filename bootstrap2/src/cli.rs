// CLI command implementations. Mirrors docs2/frontends/cli.md.
use crate::context::{self, Focus};
use crate::llm::{self, Llm};
use crate::project::{self, Project};
use crate::reconcile;
use crate::store::Store;
use crate::turn::{Trace, TraceLevel};
use std::path::PathBuf;

pub struct Options {
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub out: Option<String>,
    pub verbose: bool,
    pub quiet: bool,
    pub write: bool,
    pub focus: Option<String>,
    pub budget: Option<usize>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            base_url: None,
            model: None,
            api_key: None,
            out: None,
            verbose: false,
            quiet: false,
            write: false,
            focus: None,
            budget: None,
        }
    }
}

// Resolve the project (walking up to jazyk.toml, or ad hoc with explicit paths), the LLM
// (flag → env → global config → project → default), and the out directory.
pub fn resolve(paths: &[String], opts: &Options) -> (Project, Llm, PathBuf) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut proj = match project::find_root(&cwd) {
        Some(root) => Project::load(&root),
        None => {
            let mut p = Project::default();
            p.root = cwd.clone();
            p
        }
    };
    if !paths.is_empty() {
        let files: Vec<PathBuf> = paths
            .iter()
            .map(|p| {
                let pb = PathBuf::from(p);
                if pb.is_absolute() {
                    pb
                } else {
                    cwd.join(pb)
                }
            })
            .collect();
        proj.explicit_files = Some(files);
    }

    let global = project::load_global_llm();
    let base_url = opts
        .base_url
        .clone()
        .or_else(|| std::env::var("JAZYK_LLM_BASE_URL").ok())
        .or(global.base_url)
        .unwrap_or_else(|| proj.llm.base_url.clone());
    let model = opts
        .model
        .clone()
        .or_else(|| std::env::var("JAZYK_MODEL").ok())
        .or(global.model)
        .unwrap_or_else(|| proj.llm.model.clone());
    let api_key = opts
        .api_key
        .clone()
        .or_else(|| std::env::var(&proj.llm.api_key_env).ok())
        .or_else(|| global.api_key_env.and_then(|e| std::env::var(e).ok()))
        .or(global.api_key)
        .unwrap_or_default();
    let temperature = std::env::var("JAZYK_TEMPERATURE")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .or(global.temperature)
        .or(Some(0.0));
    let temperature = temperature.filter(|t| *t >= 0.0);

    let out = opts
        .out
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| proj.root.join("jazyk-out"));
    (proj, Llm { base_url, model, api_key, temperature }, out)
}

fn trace_for(opts: &Options) -> Trace {
    Trace {
        level: if opts.quiet {
            TraceLevel::Quiet
        } else if opts.verbose {
            TraceLevel::Verbose
        } else {
            TraceLevel::Normal
        },
    }
}

fn print_report(r: &reconcile::BuildReport) {
    println!(
        "jazyk: {} — {} dirty doc(s), {} turn(s), {} mutation(s), {} parked; {} error(s), {} warning(s); coverage {}%",
        r.verdict, r.dirty_docs, r.turns, r.applied, r.parked, r.errors, r.warnings, r.coverage_pct
    );
}

pub fn run_compile(paths: &[String], opts: &Options) -> i32 {
    let (proj, llm, out) = resolve(paths, opts);
    if opts.verbose {
        llm::set_verbose(true);
    }
    let trace = trace_for(opts);
    let report = reconcile::compile(&proj, &llm, &out, &trace);
    print_report(&report);
    if report.verdict == "converged" {
        0
    } else {
        1
    }
}

pub fn run_check(paths: &[String], opts: &Options) -> i32 {
    let (proj, llm, out) = resolve(paths, opts);
    let trace = trace_for(opts);
    let report = reconcile::compile(&proj, &llm, &out, &trace);
    print_report(&report);
    let store = Store::load(&out);
    let mut errors = 0;
    for d in store.graph.diagnostics.values() {
        if d.lifecycle == "open" && d.severity == "error" && d.triage.as_deref() != Some("suppressed") {
            errors += 1;
            eprintln!("error[{}]: {} ({})", d.rule, d.message, d.subjects.join(", "));
        }
    }
    if errors > 0 || report.verdict != "converged" {
        1
    } else {
        0
    }
}

pub fn run_watch(paths: &[String], opts: &Options) -> i32 {
    let (proj, _llm, _out) = resolve(paths, opts);
    let fingerprint = |proj: &Project| -> String {
        let mut s = String::new();
        for f in proj.doc_files() {
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
    let mut last = String::new();
    println!("jazyk: watching {} (Ctrl-C to stop)", proj.root.display());
    loop {
        let fp = fingerprint(&proj);
        if fp != last {
            last = fp;
            run_compile(paths, opts);
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

pub fn run_status(paths: &[String], opts: &Options) -> i32 {
    let (_proj, _llm, out) = resolve(paths, opts);
    let store = Store::load(&out);
    let s = &store.status;
    println!("generation: {}", s.generation);
    println!("verdict:    {}", if s.verdict.is_empty() { "(no build yet)" } else { &s.verdict });
    println!(
        "graph:      {} entities, {} requirements, {} relationships",
        store.graph.entities.len(),
        store.graph.requirements.len(),
        store.graph.relationships.len()
    );
    let (mut total, mut covered) = (0usize, 0usize);
    for rec in store.docs.values() {
        for (r, sec) in &rec.sections {
            if sec.raw.lines().skip(1).all(|l| l.trim().is_empty()) {
                continue;
            }
            total += 1;
            if rec.coverage.contains_key(r) {
                covered += 1;
            }
        }
    }
    println!("coverage:   {}/{} sections", covered, total);
    let mut by_sev: std::collections::BTreeMap<&str, usize> = Default::default();
    for d in store.graph.diagnostics.values() {
        if d.lifecycle == "open" {
            *by_sev.entry(d.severity.as_str()).or_default() += 1;
        }
    }
    println!(
        "diagnostics: {}",
        if by_sev.is_empty() {
            "none".to_string()
        } else {
            by_sev.iter().map(|(k, v)| format!("{} {}", v, k)).collect::<Vec<_>>().join(", ")
        }
    );
    println!(
        "spent:      {} turns, {} rounds, {} tokens",
        s.spent.turns, s.spent.rounds, s.spent.tokens
    );
    if !s.parked.is_empty() {
        println!("parked:");
        for p in &s.parked {
            println!("  - {} {}", p.task, p.target);
        }
    }
    0
}

pub fn run_context(paths: &[String], opts: &Options, target: &str) -> i32 {
    let (_proj, _llm, out) = resolve(paths, opts);
    let store = Store::load(&out);
    let focus = opts.focus.as_deref().map(Focus::parse).unwrap_or_default();
    let budget = opts.budget.unwrap_or(12_000);
    if target.starts_with("h:") {
        match context::expand(&store, target, budget) {
            Ok(pack) => {
                println!("{}", pack.pack);
                0
            }
            Err(e) => {
                eprintln!("jazyk: {}", e);
                1
            }
        }
    } else {
        match context::assemble(&store, target, &focus, budget) {
            Ok(pack) => {
                println!("{}", pack.pack);
                0
            }
            Err(e) => {
                eprintln!("jazyk: {}", e);
                1
            }
        }
    }
}

pub fn run_query(paths: &[String], opts: &Options, query: &str) -> i32 {
    let (_proj, _llm, out) = resolve(paths, opts);
    let store = Store::load(&out);
    let hits = store.search(query);
    if hits.is_empty() {
        println!("no matches");
        return 1;
    }
    for (id, name, def) in hits {
        println!("{} ({}): {}", id, name, def);
    }
    0
}
