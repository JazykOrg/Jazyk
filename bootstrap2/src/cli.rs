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
    pub kind: Option<String>,
    pub list: bool,
    pub audit: bool,
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
            kind: None,
            list: false,
            audit: false,
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

// The built-in generation worker: consumes the same task packages an external MCP
// worker gets (gen_pending decides what runs, gen_task supplies the package, gen_mark
// records the manifest). One task per entity produces the product files and the tests.
// Mirrors docs2/consumers/gen.md.
pub fn run_gen(opts: &Options, entities: &[String]) -> i32 {
    let (proj, llm, out) = resolve(&[], opts);
    if opts.verbose {
        llm::set_verbose(true);
    }
    let store = Store::load(&out);
    let mut gs = crate::gen::GenSettings::resolve(&proj, &out);
    if let Some(l) = &opts.lang {
        gs.lang = l.clone();
    }

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

    std::fs::create_dir_all(&gs.deliverable).ok();
    let pending: std::collections::BTreeSet<String> = crate::gen::pending(&store, &gs)
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
        let task = match crate::gen::task_package(&store, id, &gs) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("jazyk: {}: {}", id, e);
                failures += 1;
                continue;
            }
        };
        match gen_one(&store, &llm, &gs, id, &task) {
            Ok(files) => {
                println!("jazyk: generated {} ({} file(s))", id, files);
                regenerated += 1;
            }
            Err(e) => {
                eprintln!("jazyk: {} failed: {}", id, e);
                failures += 1;
            }
        }
    }
    scaffold_deliverable(&gs);
    println!(
        "jazyk: gen done — {} regenerated, {} unchanged, {} failure(s)",
        regenerated, skipped, failures
    );
    if failures > 0 {
        1
    } else {
        0
    }
}

// One entity's task: product file (in parts when dense), then the tests file, then the
// manifest mark. Requirements whose test name is absent from the tests artifact become
// llm rows with a criteria file.
fn gen_one(store: &Store, llm: &llm::Llm, gs: &crate::gen::GenSettings, id: &str, task: &serde_json::Value) -> Result<usize, String> {
    let instructions = task["instructions"].as_str().unwrap_or_default();
    let product_rel = task["suggestedLayout"]["product"].as_str().unwrap_or("src/out.txt").to_string();
    let tests_rel = task["suggestedLayout"]["tests"].as_str().unwrap_or("tests/out.txt").to_string();
    let header = format!(
        "Entity {} ({})\nContext:\n{}\nChanged since last generation: {}\nAlready generated files: {}\n",
        id,
        task["name"].as_str().unwrap_or_default(),
        task["context"].as_str().unwrap_or_default(),
        task["changed"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
        serde_json::to_string(&task["generatedFiles"]).unwrap_or_default(),
    );
    let groups = task["requirementGroups"].as_array().cloned().unwrap_or_default();
    let parts = groups.len();
    let req_line = |r: &serde_json::Value| {
        format!(
            "- {} [{}]: {}\n  quote: {}",
            r["id"].as_str().unwrap_or(""),
            r["testName"].as_str().unwrap_or(""),
            r["ears"].as_str().unwrap_or(""),
            r["quote"].as_str().unwrap_or("")
        )
    };

    // Product file, dense entities in parts.
    let mut code = String::new();
    for (k, group) in groups.iter().enumerate() {
        let req_lines: Vec<String> = group.as_array().map(|a| a.iter().map(req_line).collect()).unwrap_or_default();
        let user = if k == 0 {
            format!(
                "{}\nWrite the product file `{}`. Implement every requirement with a marker comment (// req:<id> hash:<hash8> plus the quote) at each implementing site. Requirements (group 1 of {}):\n{}\n",
                header, product_rel, parts, req_lines.join("\n")
            )
        } else {
            format!(
                "{}\nRequirements (group {} of {}):\n{}\n\nThe product file so far:\n{}\nReturn ONLY additional content to append.",
                header, k + 1, parts, req_lines.join("\n"), crate::llm::truncate(&code, 20_000)
            )
        };
        let part = llm
            .chat(instructions, &user, &format!("gen {} product {}/{}", id, k + 1, parts))
            .map_err(|e| format!("product part {}/{}: {}", k + 1, parts, e))?;
        if k > 0 {
            code.push_str(&format!("\n// --- generated part {} ---\n", k + 1));
        }
        code.push_str(&strip_fences(&part));
        code.push('\n');
    }
    if code.trim().is_empty() {
        return Err("empty product".into());
    }
    let product_path = gs.deliverable.join(&product_rel);
    if let Some(p) = product_path.parent() {
        std::fs::create_dir_all(p).ok();
    }
    std::fs::write(&product_path, &code).map_err(|e| e.to_string())?;

    // Tests file: one test per requirement, named by the suggested testName. The model
    // may omit requirements it cannot test programmatically; those become llm rows.
    let all_reqs: Vec<serde_json::Value> = groups.iter().flat_map(|g| g.as_array().cloned().unwrap_or_default()).collect();
    let req_lines: Vec<String> = all_reqs.iter().map(req_line).collect();
    let import_hint = if gs.lang == "rust" {
        format!(
            " (import it as `{}::{}`)",
            deliverable_crate_name(gs),
            std::path::Path::new(&product_rel).file_stem().map(|s| s.to_string_lossy().replace('-', "_")).unwrap_or_default()
        )
    } else {
        String::new()
    };
    let tests_user = format!(
        "{}\nWrite the tests file `{}` for the product file `{}`{}. One test per requirement, function named EXACTLY by the [testName] shown, with the marker comment (// req:<id> hash:<hash8> plus the quote) above it. If a requirement cannot be tested programmatically, omit its test; it will be verified by an agent instead. Requirements:\n{}\n\nThe product file:\n{}\n",
        header,
        tests_rel,
        product_rel,
        import_hint,
        req_lines.join("\n"),
        crate::llm::truncate(&code, 16_000)
    );
    // A failed tests call fails the task: silently demoting every requirement to llm
    // would misreport the worker's actual coverage.
    let tests_code = if run_template(&gs.lang, "x").is_some() {
        llm.chat(instructions, &tests_user, &format!("gen {} tests", id))
            .map(|t| strip_fences(&t))
            .map_err(|e| format!("tests file: {}", e))?
    } else {
        String::new()
    };
    let mut files = vec![product_rel.clone()];
    if !tests_code.trim().is_empty() {
        let tests_path = gs.deliverable.join(&tests_rel);
        if let Some(p) = tests_path.parent() {
            std::fs::create_dir_all(p).ok();
        }
        std::fs::write(&tests_path, &tests_code).map_err(|e| e.to_string())?;
        files.push(tests_rel.clone());
    }

    // Manifest: programmatic rows for tests present in the artifact, llm rows (with a
    // criteria file) for the rest.
    let mut tests_manifest: Vec<serde_json::Value> = Vec::new();
    for r in &all_reqs {
        let rid = r["id"].as_str().unwrap_or_default();
        let name = r["testName"].as_str().unwrap_or_default();
        let template = run_template(&gs.lang, name);
        if tests_code.contains(name) && template.is_some() {
            tests_manifest.push(serde_json::json!({
                "requirement": rid, "kind": "programmatic", "label": "unit",
                "artifact": tests_rel, "name": name,
                "run": template.unwrap(),
                "files": [product_rel],
            }));
        } else {
            let crit_rel = r["criteriaPath"].as_str().unwrap_or_default().to_string();
            let crit_path = store.out.join("gen").join(&crit_rel);
            if let Some(p) = crit_path.parent() {
                std::fs::create_dir_all(p).ok();
            }
            let criteria = format!(
                "---\nrequirement: {}\nhash: {}\n---\n\n# Verify {}\n\nStatement: {}\n\n> {}\n\nImplementing files (under the deliverable):\n- {}\n\nConfirm the statement holds in the implementation. Verdict contract: reply PASS or FAIL with reasoning.\n",
                rid,
                r["hash"].as_str().unwrap_or_default(),
                rid,
                r["ears"].as_str().unwrap_or_default(),
                r["quote"].as_str().unwrap_or_default(),
                product_rel,
            );
            std::fs::write(&crit_path, criteria).ok();
            tests_manifest.push(serde_json::json!({
                "requirement": rid, "kind": "llm", "label": "llm",
                "artifact": crit_rel, "name": name,
                "run": format!("jazyk test {}", rid),
                "files": [product_rel],
            }));
        }
    }
    let manifest = serde_json::json!({"files": files, "tests": tests_manifest});
    crate::gen::mark(store, id, task["factHash"].as_str(), &manifest, gs)?;
    Ok(files.len())
}

// The default command that runs exactly one named test, per lang. None means the
// built-in worker has no programmatic runner for the lang and records llm rows.
fn run_template(lang: &str, name: &str) -> Option<String> {
    match lang {
        "rust" => Some(format!("cargo test {}", name)),
        "python" => Some(format!("python3 -m pytest -k {} -q", name)),
        "typescript" => Some(format!("npx vitest run -t {}", name)),
        "go" => Some(format!("go test -run {} ./...", name)),
        _ => None,
    }
}

fn deliverable_crate_name(gs: &crate::gen::GenSettings) -> String {
    gs.deliverable
        .file_name()
        .map(|s| s.to_string_lossy().replace('-', "_"))
        .unwrap_or_else(|| "product".into())
}

// Make a rust deliverable runnable: a Cargo.toml and a src/lib.rs naming every module.
// Deterministic scaffold, not attributed to any entity.
fn scaffold_deliverable(gs: &crate::gen::GenSettings) {
    if gs.lang != "rust" {
        return;
    }
    let src = gs.deliverable.join("src");
    if !src.exists() {
        return;
    }
    let cargo = gs.deliverable.join("Cargo.toml");
    if !cargo.exists() {
        std::fs::write(
            &cargo,
            format!(
                "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ndoctest = false\n",
                deliverable_crate_name(gs)
            ),
        )
        .ok();
    }
    let mut mods: Vec<String> = std::fs::read_dir(&src)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().to_string();
                    n.strip_suffix(".rs").filter(|s| *s != "lib").map(|s| s.replace('-', "_"))
                })
                .collect()
        })
        .unwrap_or_default();
    mods.sort();
    let lib: String = mods.iter().map(|m| format!("pub mod {};\n", m)).collect();
    std::fs::write(src.join("lib.rs"), format!("// generated scaffold: one module per entity\n{}", lib)).ok();
}

// Verification: run the ledger's tests and record verdicts. Programmatic rows execute
// their command; llm rows drive the configured model against the verify_task package.
// Mirrors docs2/consumers/gen.md#runners.
pub fn run_test(opts: &Options, targets: &[String]) -> i32 {
    let (proj, llm, out) = resolve(&[], opts);
    if opts.verbose {
        llm::set_verbose(true);
    }
    let store = Store::load(&out);
    let mut gs = crate::gen::GenSettings::resolve(&proj, &out);
    if let Some(l) = &opts.lang {
        gs.lang = l.clone();
    }
    if opts.audit {
        let r = crate::verify::audit(&store, &gs);
        println!("jazyk: audit — {}", r);
        return 0;
    }
    let filter = if opts.force { "all" } else { "stale" };
    let rows = crate::verify::pending(&store, &gs, Some(filter), None);
    // Target filtering: entity ids select their rows, requirement ids select directly.
    let selected: Vec<&serde_json::Value> = rows
        .iter()
        .filter(|r| {
            if targets.is_empty() {
                return true;
            }
            targets.iter().any(|t| {
                let t = store.resolve_id(t);
                r["requirement"].as_str() == Some(t) || store.resolve_id(r["entity"].as_str().unwrap_or("")) == t
            })
        })
        .filter(|r| match &opts.kind {
            Some(k) => r["test"]["kind"].as_str() == Some(k.as_str()),
            None => true,
        })
        .collect();
    if !targets.is_empty() && selected.is_empty() {
        eprintln!("jazyk: no ledger rows match the given target(s); run `jazyk gen` first or check the ids");
        return 1;
    }
    if opts.list {
        for r in &selected {
            println!(
                "{:24} {:18} {:13} {} ({})",
                r["requirement"].as_str().unwrap_or(""),
                r["status"].as_str().unwrap_or(""),
                r["test"]["kind"].as_str().unwrap_or(""),
                r["test"]["run"].as_str().unwrap_or(""),
                r["reason"].as_str().unwrap_or("")
            );
        }
        println!("jazyk: {} row(s)", selected.len());
        return 0;
    }
    let (mut verified, mut failing, mut stale, mut skipped) = (0, 0, 0, 0);
    for r in &selected {
        let rid = r["requirement"].as_str().unwrap_or_default().to_string();
        let status = r["status"].as_str().unwrap_or_default();
        if status == "stale-requirement" || status == "missing" {
            eprintln!("jazyk: {} is {} ({}); generate with `jazyk gen {}`", rid, status, r["reason"].as_str().unwrap_or(""), r["entity"].as_str().unwrap_or(""));
            stale += 1;
            continue;
        }
        let kind = r["test"]["kind"].as_str().unwrap_or("programmatic");
        let verdict = if kind == "llm" {
            match crate::verify::task(&store, &rid, &gs) {
                Ok(task) => {
                    let mut evidence_input = String::new();
                    for f in task["files"].as_array().cloned().unwrap_or_default() {
                        if let Some(f) = f.as_str() {
                            if let Ok(content) = std::fs::read_to_string(gs.deliverable.join(f)) {
                                evidence_input.push_str(&format!("\n=== {} ===\n{}", f, crate::llm::truncate(&content, 12_000)));
                            }
                        }
                    }
                    let user = format!(
                        "{}\n\nContext:\n{}\n\nImplementing files:{}\n\nReply with a verdict line `PASS` or `FAIL`, then your reasoning.",
                        task["criteria"].as_str().unwrap_or_default(),
                        task["context"].as_str().unwrap_or_default(),
                        evidence_input
                    );
                    match llm.chat(task["instructions"].as_str().unwrap_or_default(), &user, &format!("verify {}", rid)) {
                        Ok(reply) => {
                            let up = reply.to_uppercase();
                            match (up.find("PASS"), up.find("FAIL")) {
                                (Some(p), Some(f)) => Some((p < f, crate::llm::truncate(reply.trim(), 300).to_string())),
                                (Some(_), None) => Some((true, crate::llm::truncate(reply.trim(), 300).to_string())),
                                (None, Some(_)) => Some((false, crate::llm::truncate(reply.trim(), 300).to_string())),
                                (None, None) => {
                                    eprintln!("jazyk: {} verdict unparseable (no PASS or FAIL in reply)", rid);
                                    None
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("jazyk: {} llm run failed: {}", rid, e);
                            None
                        }
                    }
                }
                Err(e) => {
                    eprintln!("jazyk: {}: {}", rid, e);
                    None
                }
            }
        } else {
            match crate::verify::run_programmatic(&store, &rid, &gs) {
                Ok((pass, evidence)) => Some((pass, evidence)),
                Err(e) => {
                    eprintln!("jazyk: {}: {}", rid, e);
                    None
                }
            }
        };
        match verdict {
            Some((pass, evidence)) => {
                let v = if pass { "pass" } else { "fail" };
                crate::verify::mark(&store, &rid, v, None, Some(&evidence), &gs).ok();
                println!("jazyk: {} {} ({})", rid, if pass { "verified" } else { "FAILING" }, r["test"]["run"].as_str().unwrap_or(""));
                if pass {
                    verified += 1;
                } else {
                    failing += 1;
                }
            }
            None => skipped += 1,
        }
    }
    if selected.is_empty() {
        println!("jazyk: nothing to do; every targeted row is verified");
    } else {
        println!(
            "jazyk: test done — {} verified, {} failing, {} stale, {} skipped",
            verified, failing, stale, skipped
        );
    }
    if failing > 0 || stale > 0 || skipped > 0 {
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
