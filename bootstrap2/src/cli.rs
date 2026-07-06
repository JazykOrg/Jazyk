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
    // Native file events via the notify crate; the fingerprint over the matched doc
    // files decides whether a build actually runs, so editor temp files, renames, and
    // the out directory's own writes never trigger one.
    use notify::Watcher;
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let mut watcher = match notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
        if res.is_ok() {
            tx.send(()).ok();
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("jazyk: file watcher unavailable ({}); falling back to polling", e);
            let mut last = String::new();
            loop {
                let fp = fingerprint(&proj);
                if fp != last {
                    last = fp;
                    run_compile(paths, opts);
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
    let mut last = fingerprint(&proj);
    run_compile(paths, opts);
    loop {
        if rx.recv().is_err() {
            break;
        }
        // Debounce: editors save in bursts; let the burst finish.
        std::thread::sleep(std::time::Duration::from_millis(300));
        while rx.try_recv().is_ok() {}
        let fp = fingerprint(&proj);
        if fp != last {
            last = fp;
            run_compile(paths, opts);
        }
    }
    0
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
// records the manifest). The model owns every choice about the deliverable: the medium
// (derived from the context), the file paths, and the run commands. The harness only
// writes what the model returns, validates the manifest deterministically, and records
// it. Mirrors docs2/consumers/gen.md.
pub fn run_gen(opts: &Options, entities: &[String]) -> i32 {
    let (proj, llm, out) = resolve(&[], opts);
    if opts.verbose {
        llm::set_verbose(true);
    }
    let store = Store::load(&out);
    let gs = crate::gen::GenSettings::resolve(&proj, &out);

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

// Parse a reply whose first line must be `FILE: <relative path>`; returns (path, body).
fn parse_file_reply(reply: &str) -> Result<(String, String), String> {
    let reply = strip_fences(reply);
    let mut lines = reply.splitn(2, '\n');
    let first = lines.next().unwrap_or("").trim();
    let Some(path) = first.strip_prefix("FILE:") else {
        return Err(format!("first line must be `FILE: <path>`, got `{}`", crate::llm::truncate(first, 80)));
    };
    let path = path.trim();
    if path.is_empty() || path.starts_with('/') || path.contains("..") {
        return Err(format!("bad file path `{}`", path));
    }
    Ok((path.to_string(), lines.next().unwrap_or("").to_string()))
}

// One entity's task: the model picks the product path (FILE protocol, parts when
// dense), the tests path, and the manifest with the run commands. Requirements the
// model declares untestable programmatically, or whose declared test fails validation
// (name missing from the artifact, empty command), become llm rows with a criteria
// file. Manifest validation is deterministic; nothing here chooses for the model.
fn gen_one(store: &Store, llm: &llm::Llm, gs: &crate::gen::GenSettings, id: &str, task: &serde_json::Value) -> Result<usize, String> {
    let instructions = task["instructions"].as_str().unwrap_or_default();
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

    // Product content; the model names the file.
    let mut code = String::new();
    let mut product_rel = String::new();
    for (k, group) in groups.iter().enumerate() {
        let req_lines: Vec<String> = group.as_array().map(|a| a.iter().map(req_line).collect()).unwrap_or_default();
        let user = if k == 0 {
            format!(
                "{}\nWrite the implementing content for this entity. Derive the medium from the context; choose the file path yourself, relative to the deliverable. Reply with the first line exactly `FILE: <path>` and the file content after it. Put the marker comment (// req:<id> hash:<hash8> plus the quote, in the medium's comment syntax) at each implementing site. Requirements (group 1 of {}):\n{}\n",
                header, parts, req_lines.join("\n")
            )
        } else {
            format!(
                "{}\nRequirements (group {} of {}):\n{}\n\nThe file `{}` so far:\n{}\nReturn ONLY additional content to append, no FILE line.",
                header, k + 1, parts, req_lines.join("\n"), product_rel, crate::llm::truncate(&code, 20_000)
            )
        };
        let reply = llm
            .chat(instructions, &user, &format!("gen {} product {}/{}", id, k + 1, parts))
            .map_err(|e| format!("product part {}/{}: {}", k + 1, parts, e))?;
        if k == 0 {
            let (path, body) = parse_file_reply(&reply)?;
            product_rel = path;
            code = body;
        } else {
            code.push_str("\n");
            code.push_str(&strip_fences(&reply));
        }
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

    // Tests: the model names the file too, or declares nothing programmatic.
    let all_reqs: Vec<serde_json::Value> = groups.iter().flat_map(|g| g.as_array().cloned().unwrap_or_default()).collect();
    let req_lines: Vec<String> = all_reqs.iter().map(req_line).collect();
    let tests_user = format!(
        "{}\nWrite the tests for the requirements against `{}` (content below). Choose the test file path yourself. One test per requirement you can test programmatically, named EXACTLY by its [testName], with the marker comment above it. Reply with the first line exactly `FILE: <path>` and the content after it. If no requirement can be tested programmatically, reply with exactly `NONE`. Requirements:\n{}\n\nThe product file:\n{}\n",
        header,
        product_rel,
        req_lines.join("\n"),
        crate::llm::truncate(&code, 16_000)
    );
    let tests_reply = llm
        .chat(instructions, &tests_user, &format!("gen {} tests", id))
        .map_err(|e| format!("tests file: {}", e))?;
    let mut files = vec![product_rel.clone()];
    let mut tests_rel = String::new();
    let mut tests_code = String::new();
    if tests_reply.trim() != "NONE" {
        let (path, body) = parse_file_reply(&tests_reply).map_err(|e| format!("tests reply: {}", e))?;
        tests_rel = path;
        tests_code = body;
        let tests_path = gs.deliverable.join(&tests_rel);
        if let Some(p) = tests_path.parent() {
            std::fs::create_dir_all(p).ok();
        }
        std::fs::write(&tests_path, &tests_code).map_err(|e| e.to_string())?;
        files.push(tests_rel.clone());
    }

    // The manifest: the model declares run commands and any support files it needs;
    // support files are returned inline and written here.
    let manifest_user = format!(
        "Files written so far for entity {}: {:?} under the deliverable directory `{}`.\nReturn ONLY a JSON object, no prose:\n{{\"supportFiles\": [{{\"path\": \"...\", \"content\": \"...\"}}], \"tests\": [{{\"requirement\": \"req:...\", \"kind\": \"programmatic\"|\"llm\", \"label\": \"your words\", \"name\": \"the testName\", \"run\": \"exact command executed from the deliverable directory that runs only that test\", \"cwd\": \".\"}}]}}\nsupportFiles are build or configuration files required for the run commands to execute (empty array if none are needed or they already exist). Every requirement must appear once in tests. Requirements and test names:\n{}\n\nThe tests file `{}`:\n{}\n",
        id,
        files,
        task["deliverable"].as_str().unwrap_or_default(),
        all_reqs.iter().map(|r| format!("- {} [{}]", r["id"].as_str().unwrap_or(""), r["testName"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("\n"),
        tests_rel,
        crate::llm::truncate(&tests_code, 12_000)
    );
    let manifest_reply = llm
        .chat(instructions, &manifest_user, &format!("gen {} manifest", id))
        .map_err(|e| format!("manifest: {}", e))?;
    let manifest_json: serde_json::Value = {
        let text = strip_fences(&manifest_reply);
        let start = text.find('{').ok_or("manifest reply held no JSON object")?;
        let end = text.rfind('}').ok_or("manifest reply held no JSON object")?;
        serde_json::from_str(&text[start..=end]).map_err(|e| format!("manifest JSON: {}", e))?
    };
    if let Some(support) = manifest_json["supportFiles"].as_array() {
        for f in support {
            let (Some(path), Some(content)) = (f["path"].as_str(), f["content"].as_str()) else { continue };
            if path.starts_with('/') || path.contains("..") {
                continue;
            }
            let p = gs.deliverable.join(path);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(&p, content).map_err(|e| e.to_string())?;
            files.push(path.to_string());
        }
    }

    // Deterministic validation: a programmatic row needs its declared test present in
    // the tests artifact and a non-empty command; anything else becomes an llm row.
    let declared = manifest_json["tests"].as_array().cloned().unwrap_or_default();
    let mut tests_manifest: Vec<serde_json::Value> = Vec::new();
    for r in &all_reqs {
        let rid = r["id"].as_str().unwrap_or_default();
        let name = r["testName"].as_str().unwrap_or_default();
        let row = declared.iter().find(|t| t["requirement"].as_str() == Some(rid));
        let programmatic = row
            .map(|t| {
                t["kind"].as_str() == Some("programmatic")
                    && !t["run"].as_str().unwrap_or("").trim().is_empty()
                    && tests_code.contains(name)
            })
            .unwrap_or(false);
        if programmatic {
            let t = row.unwrap();
            tests_manifest.push(serde_json::json!({
                "requirement": rid, "kind": "programmatic",
                "label": t["label"].as_str().unwrap_or("test"),
                "artifact": tests_rel, "name": name,
                "run": t["run"].as_str().unwrap_or(""),
                "cwd": t["cwd"].as_str().unwrap_or("."),
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
                "requirement": rid, "kind": "llm",
                "label": row.and_then(|t| t["label"].as_str()).unwrap_or("llm"),
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

// Verification: run the ledger's tests and record verdicts. Programmatic rows execute
// their command; llm rows drive the configured model against the verify_task package.
// Mirrors docs2/consumers/gen.md#runners.
pub fn run_test(opts: &Options, targets: &[String]) -> i32 {
    let (proj, llm, out) = resolve(&[], opts);
    if opts.verbose {
        llm::set_verbose(true);
    }
    let store = Store::load(&out);
    let gs = crate::gen::GenSettings::resolve(&proj, &out);
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
    let (proj, _llm, out) = resolve(&[], &store_opts);
    let store = Store::load(&out);
    let gs = crate::gen::GenSettings::resolve(&proj, &out);
    let html = crate::viewer::render(&store, &gs);
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
