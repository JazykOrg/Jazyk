// CLI command implementations. Mirrors docs2/frontends/cli.md.
use crate::context::{self, Focus};
use crate::llm::{self, Llm};
use crate::project::{self, Project};
use crate::reconcile;
use crate::store::Store;
use crate::turn::{Trace, TraceLevel};
use std::path::PathBuf;

#[derive(Clone)]
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
    pub lang: Option<String>,
    pub force: bool,
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
            lang: None,
            force: false,
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

// Generate one code unit per entity from its assembled context pack. Leaf entities
// (fewest ungenerated neighbors) go first, so later units can reference earlier ones.
pub fn run_codegen(opts: &Options, entities: &[String]) -> i32 {
    let (_proj, llm, out) = resolve(&[], opts);
    if opts.verbose {
        llm::set_verbose(true);
    }
    let store = Store::load(&out);
    let lang = opts.lang.clone().unwrap_or_else(|| "rust".to_string());
    let ext = match lang.as_str() {
        "rust" => "rs",
        "python" => "py",
        "typescript" => "ts",
        "go" => "go",
        _ => "txt",
    };

    let mut targets: Vec<String> = if entities.is_empty() {
        store
            .graph
            .entities
            .keys()
            .filter(|id| !store.requirements_referencing(id).is_empty())
            .cloned()
            .collect()
    } else {
        let mut v = Vec::new();
        for e in entities {
            let id = store.resolve_id(e).to_string();
            if store.graph.entities.contains_key(&id) {
                v.push(id);
            } else {
                eprintln!("jazyk: unknown entity `{}`", e);
                return 2;
            }
        }
        v
    };
    if targets.is_empty() {
        eprintln!("jazyk: no entities with requirements; run `jazyk compile` first");
        return 1;
    }

    // Leaf-first ordering: repeatedly emit the entity with the fewest ungenerated
    // neighbors over the relationship graph (ties by name).
    let neighbors = |id: &str| -> Vec<String> {
        store
            .graph
            .relationships
            .values()
            .filter(|r| r.members.iter().any(|m| m == id))
            .flat_map(|r| r.members.iter().filter(|m| *m != id).cloned())
            .collect()
    };
    let mut ordered: Vec<String> = Vec::new();
    while !targets.is_empty() {
        let (i, _) = targets
            .iter()
            .enumerate()
            .min_by_key(|(_, id)| {
                let pending = neighbors(id).iter().filter(|n| targets.contains(n)).count();
                (pending, (*id).clone())
            })
            .unwrap();
        ordered.push(targets.remove(i));
    }

    std::fs::create_dir_all(out.join("codegen")).ok();
    // The built-in worker consumes the same task packages an external MCP worker gets:
    // codegen_pending decides what runs, codegen_task supplies the package, codegen_mark
    // records completion. See docs2/consumers/codegen.md#pluggable-workers.
    let pending: std::collections::BTreeSet<String> = crate::gen::pending(&store, &lang)
        .iter()
        .filter_map(|p| p["entity"].as_str().map(String::from))
        .collect();
    let mut regenerated = 0;
    let mut skipped = 0;
    let mut failures = 0;
    for id in &ordered {
        if !opts.force && !pending.contains(id) {
            skipped += 1;
            continue;
        }
        let task = match crate::gen::task_package(&store, id, &lang) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("jazyk: {}: {}", id, e);
                failures += 1;
                continue;
            }
        };
        let instructions = task["instructions"].as_str().unwrap_or_default();
        let header = format!(
            "Entity {} ({})\nContext:\n{}\nRelationships: {}\nChanged since last generation: {}\nAlready generated units: {}\n",
            id,
            task["name"].as_str().unwrap_or_default(),
            task["context"].as_str().unwrap_or_default(),
            task["relationships"].as_array().map(|a| a.len()).unwrap_or(0),
            task["changed"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
            task["generatedUnits"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
        );
        let groups = task["requirementGroups"].as_array().cloned().unwrap_or_default();
        let parts = groups.len();
        let mut code = String::new();
        let mut ok = true;
        for (k, group) in groups.iter().enumerate() {
            let req_lines: Vec<String> = group
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|r| format!("- {}: {}", r["id"].as_str().unwrap_or(""), r["ears"].as_str().unwrap_or("")))
                        .collect()
                })
                .unwrap_or_default();
            let user = if k == 0 {
                format!("{}\nRequirements (group 1 of {}):\n{}\n", header, parts, req_lines.join("\n"))
            } else {
                format!(
                    "{}\nRequirements (group {} of {}):\n{}\n\nModule so far:\n{}\n",
                    header,
                    k + 1,
                    parts,
                    req_lines.join("\n"),
                    crate::llm::truncate(&code, 20_000)
                )
            };
            match llm.chat(instructions, &user, &format!("codegen {} part {}/{}", id, k + 1, parts)) {
                Ok(part) => {
                    if k > 0 {
                        code.push_str(&format!("\n// --- generated part {} ---\n", k + 1));
                    }
                    code.push_str(&strip_fences(&part));
                    code.push('\n');
                }
                Err(err) => {
                    eprintln!("jazyk: {} part {}/{} failed: {}", id, k + 1, parts, err);
                    ok = false;
                    break;
                }
            }
        }
        if ok && !code.trim().is_empty() {
            let path = std::path::PathBuf::from(task["unit"].as_str().unwrap_or_default());
            std::fs::write(&path, &code).ok();
            println!(
                "jazyk: wrote {}{}",
                path.display(),
                if parts > 1 { format!(" ({} parts)", parts) } else { String::new() }
            );
            crate::gen::mark(&store, id, task["factHash"].as_str()).ok();
            regenerated += 1;
        } else {
            failures += 1;
        }
    }
    println!(
        "jazyk: codegen done — {} regenerated, {} unchanged, {} failure(s)",
        regenerated, skipped, failures
    );
    if failures > 0 {
        1
    } else {
        0
    }
}

// Derive tests from requirements: one file per entity, one or more tests per
// requirement, the verbatim quote embedded as the trace.
pub fn run_testgen(opts: &Options, entities: &[String]) -> i32 {
    let (_proj, llm, out) = resolve(&[], opts);
    if opts.verbose {
        llm::set_verbose(true);
    }
    let store = Store::load(&out);
    let lang = opts.lang.clone().unwrap_or_else(|| "rust".to_string());
    let ext = match lang.as_str() {
        "rust" => "rs",
        "python" => "py",
        "typescript" => "ts",
        "go" => "go",
        _ => "txt",
    };
    // Group requirements by their first referenced entity so each lands in one file.
    let mut by_entity: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for (rid, r) in &store.graph.requirements {
        let Some(first) = r.entities.first() else { continue };
        let owner = store.resolve_id(first).to_string();
        if !entities.is_empty() {
            let wanted = entities.iter().any(|e| store.resolve_id(e) == owner || r.entities.iter().any(|x| store.resolve_id(x) == store.resolve_id(e)));
            if !wanted {
                continue;
            }
        }
        by_entity.entry(owner).or_default().push(rid.clone());
    }
    if by_entity.is_empty() {
        eprintln!("jazyk: no requirements to derive tests from; run `jazyk compile` first");
        return 1;
    }
    let dir = out.join("testgen");
    std::fs::create_dir_all(&dir).ok();
    let mut units = 0;
    let mut failures = 0;
    for (ent, rids) in &by_entity {
        let slug = ent.strip_prefix("ent:").unwrap_or(ent);
        let mut file = format!("// tests for {} (one test per requirement; the quote is the trace)\n", ent);
        for rid in rids {
            let Some(r) = store.graph.requirements.get(rid) else { continue };
            // The EARS pattern decides the test shape.
            let ears_l = r.ears.to_lowercase();
            let shape = if ears_l.starts_with("when") {
                "a scenario test: arrange, trigger the event, assert the response"
            } else if ears_l.starts_with("if") {
                "a negative test: set up the condition, assert the required handling"
            } else if ears_l.starts_with("while") {
                "a stateful test: enter the state, assert the behavior holds throughout"
            } else {
                "a property or invariant test"
            };
            let pack = context::assemble(&store, rid, &Focus { parents: 1, mentions: 1, requirements: 2 }, 8_000)
                .map(|p| p.pack)
                .unwrap_or_default();
            let system = format!(
                "You are the test generation consumer of jazyk, a natural language compiler. Derive {} in {} for exactly ONE requirement. The test function name must embed the requirement id (sanitized to identifier characters). Put the requirement's verbatim quote in a comment above the test. Reference the unit under test as the module named after the entity slug. Return ONLY code, no fences, no prose.",
                shape, lang
            );
            match llm.chat(&system, &pack, &format!("testgen {}", rid)) {
                Ok(code) => {
                    file.push_str(&format!("\n// --- {} ---\n{}\n", rid, strip_fences(&code)));
                }
                Err(e) => {
                    eprintln!("jazyk: {} failed: {}", rid, e);
                    failures += 1;
                }
            }
        }
        let path = dir.join(format!("{}.{}", slug, ext));
        std::fs::write(&path, &file).ok();
        println!("jazyk: wrote {} ({} requirement(s))", path.display(), rids.len());
        units += 1;
    }
    println!("jazyk: testgen done — {} file(s), {} failure(s)", units, failures);
    if failures > 0 {
        1
    } else {
        0
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
    let (_proj, _llm, out) = resolve(&[], &store_opts);
    let store = Store::load(&out);
    let html = crate::viewer::render(&store);
    let path = html_target
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| out.join("graph.html"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    match std::fs::write(&path, html) {
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

// Models wrap code in markdown fences despite instructions; strip one outer fence.
fn strip_fences(s: &str) -> String {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        if let Some(end) = rest.rfind("```") {
            let body = &rest[..end];
            return body.split_once('\n').map(|(_, b)| b).unwrap_or(body).to_string();
        }
    }
    t.to_string()
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
