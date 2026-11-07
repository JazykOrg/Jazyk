use crate::vfs::VPathExt;
// CLI command implementations. Mirrors docs/frontends/cli.md.
use crate::context;
use crate::llm::{self, Llm};
use crate::project::{self, Project};
use crate::reconcile;
use crate::session::{Trace, TraceLevel};
use crate::store::Store;
use crate::acp::config::BUILTIN_AGENTS as ACP_AGENTS;
use std::path::{Path, PathBuf};

#[derive(Clone)]
#[derive(Default)]
pub struct Options {
    pub set: Option<String>,
    pub dirty: bool,
    pub docs: Vec<String>,
    pub source: Vec<String>,
    pub roots: Vec<String>,
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub llm_api: Option<String>,
    pub reasoning_summary: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub out: Option<String>,
    pub verbose: bool,
    pub quiet: bool,
    pub write: bool,
    // `jazyk context`: neighbor depth and the handles to follow before printing.
    pub depth: Option<u32>,
    pub expand: Vec<String>,
    // `jazyk ripple --back`: walk causes instead of consequences.
    pub back: bool,
    pub force: bool,
    pub kind: Option<String>,
    pub list: bool,
    pub audit: bool,
    pub port: Option<u16>,
    pub no_open: bool,
    pub watch: bool,
    pub gui_url: Option<String>,
    pub no_token: bool,
    pub dev: bool,
    pub mcp: Option<String>,
    pub json: bool,
    // `jazyk status --costs`: the journal's cost report (docs/frontends/cli.md#jazyk-status).
    pub costs: bool,
    // `jazyk spend <generation> <tokens> --cached N`: the cached input among the
    // reported tokens (docs/frontends/cli.md#jazyk-spend).
    pub cached: Option<u64>,
    pub once: bool,
    // ACP: the agent profile (--agent), and the flags of bridge-spawned MCP servings
    // (docs/frontends/mcp.md#mcp-into-acp-sessions).
    pub agent: Option<String>,
    pub acp_ide: Option<String>,
    // `jazyk setup --remove`: setup names, or `all`.
    pub remove: Option<String>,
    pub ephemeral: bool,
    pub only: Option<String>,
    pub build_token: Option<String>,
    pub cancel_file: Option<String>,
    pub serve_files: bool,
    pub edit_sink: Option<String>,
    pub packaged: bool,
    // `jazyk benchmark --project <dir> --goal <id>`: one goal's session from a copy
    // of a real project (docs/benchmark/benchmark.md#snippets-from-a-real-project).
    pub project: Option<String>,
    pub goal: Option<String>,
    // `jazyk compile --sessions N`: at most N sessions this run.
    pub sessions: Option<usize>,
    // `jazyk answer <id> --option N | --text "..."`: the reply to a prompt
    // (docs/frontends/cli.md#jazyk-answer).
    pub option: Option<usize>,
    pub text: Option<String>,
}


// Initialize the current directory as a project root: a minimal jazyk.toml plus
// optional MCP integration for a coding agent. Warns instead of overwriting.
// Mirrors docs/frontends/cli.md#jazyk-init.
pub fn run_init(opts: &Options) -> i32 {
    // A flag naming something jazyk does not know is a usage error before any file
    // is written: a scaffold followed by a refusal would exit 1 with work done.
    if let Err(e) = init_flags_known(opts) {
        eprintln!("jazyk: {}; `jazyk init --help` lists the choices", e);
        return 2;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = cwd.join(crate::document_sets::CONFIG);
    let mut wrote_something = false;
    if path.vexists() {
        eprintln!(
            "jazyk: jazyk.toml already exists in {}; leaving it unchanged",
            cwd.display()
        );
        // An existing project still gets its AGENTS.md section on an explicit init.
        let outcome = crate::agents_md::sync(&cwd);
        for item in &outcome.items {
            println!("jazyk: wrote {}", item);
        }
        for w in &outcome.warnings {
            eprintln!("jazyk: {w}");
        }
        wrote_something |= !outcome.items.is_empty();
    } else {
        let previous = project::find_root(&cwd);
        if let Err((_, e)) = crate::document_sets::atomic_config(&cwd, INIT_TOML, &crate::model::hash_hex("")) {
            eprintln!("jazyk: cannot write {}: {}", path.display(), e);
            return 1;
        }
        println!("jazyk: initialized {}", path.display());
        wrote_something = true;
        if let Some(root) = previous {
            if root != cwd {
                println!(
                    "jazyk: note: this directory previously resolved to {}; the nearest jazyk.toml wins, so it now stands alone",
                    root.display()
                );
            }
        }
        match init_scaffold(&cwd) {
            Ok(made) => {
                for what in made {
                    println!("jazyk: created {}", what);
                }
            }
            Err(e) => {
                eprintln!("jazyk: {}", e);
                return 1;
            }
        }
    }
    if init_setup(&cwd, opts.mcp.as_deref(), opts.acp_ide.as_deref()) {
        wrote_something = true;
    }
    // Which agent does the AI work, and for the embedded one, which model.
    // Mirrors docs/frontends/cli.md#jazyk-init.
    if init_agent(&cwd, opts.agent.as_deref()) {
        wrote_something = true;
    }
    if wrote_something {
        0
    } else {
        1
    }
}

// Every `--mcp`, `--agent`, and `--acp` value names a known choice or `none`.
// Mirrors docs/frontends/cli.md#jazyk-init.
fn init_flags_known(opts: &Options) -> Result<(), String> {
    if let Some(m) = opts.mcp.as_deref() {
        list_known(m, &crate::setup::AGENTS.iter().map(|a| a.name).collect::<Vec<_>>(), "agent")?;
    }
    if let Some(a) = opts.agent.as_deref() {
        if a != "none" && !ACP_AGENTS.iter().any(|x| x.0 == a) {
            return Err(format!(
                "unknown agent `{}`; one of {}, none",
                a,
                ACP_AGENTS.iter().map(|x| x.0).collect::<Vec<_>>().join(", ")
            ));
        }
    }
    if let Some(ide) = opts.acp_ide.as_deref() {
        list_known(ide, &crate::acp::install::IDES, "editor")?;
    }
    Ok(())
}

// A `--mcp`/`--acp` value: `none`, `all`, or a comma-separated list of known names.
fn list_known(value: &str, known: &[&str], what: &str) -> Result<(), String> {
    if value == "none" || value == "all" {
        return Ok(());
    }
    for name in value.split(',').map(str::trim) {
        if !known.contains(&name) {
            return Err(format!("unknown {} `{}`; one of {}, all, none", what, name, known.join(", ")));
        }
    }
    Ok(())
}

// The starter jazyk.toml `jazyk init` (and the init_project chat tool) writes.
pub(crate) const INIT_TOML: &str = crate::document_sets::EMPTY_CONFIG;

// Scaffold the layout the fresh jazyk.toml names: docs/ with a placeholder root
// document, and the deliverable directory. Existing directories and files stay
// untouched. Returns what it created and prints nothing: the same scaffold runs
// inside the MCP serving and the ACP proxy, where stdout carries the protocol and a
// stray line of prose corrupts the stream.
pub(crate) fn init_scaffold(cwd: &std::path::Path) -> Result<Vec<String>, String> {
    crate::vfs::create_dir_all(cwd.join(".jazyk")).map_err(|e| e.to_string())?;
    crate::document_sets::ensure_ignore(cwd).map_err(|e| e.to_string())?;
    // The jazyk section of AGENTS.md (docs/frontends/agent-context.md#when-it-is-written).
    // Warnings go to stderr, which never carries a protocol.
    let outcome = crate::agents_md::sync(cwd);
    for w in &outcome.warnings {
        eprintln!("jazyk: {w}");
    }
    Ok(outcome.items)
}

// Agent setup for everything installed: one checklist of the Jazyk agent per
// installed IDE and every detected coding agent. `--mcp` and `--acp` each answer
// their half without the checklist; a non-interactive stdin skips what is left.
// Mirrors docs/frontends/cli.md#jazyk-init.
fn init_setup(cwd: &std::path::Path, mcp: Option<&str>, acp: Option<&str>) -> bool {
    use crate::setup::Item;
    use std::io::IsTerminal;
    let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let machine = crate::setup::Machine::scan();
    let all = crate::setup::items(&machine, None);
    // (row, named by a flag: naming it is the consent)
    let mut chosen: Vec<(Item, bool)> = Vec::new();
    let mut snippets: Vec<String> = Vec::new();
    let names = |v: &str| v.split(',').map(|n| n.trim().to_string()).collect::<Vec<_>>();
    match mcp {
        None => {}
        Some("none") => {}
        Some("all") => chosen.extend(all.iter().filter(|i| matches!(i, Item::Agent(_))).map(|i| (i.clone(), false))),
        Some(v) => {
            for n in names(v) {
                if let Some(a) = crate::setup::agent_by_name(&n) {
                    chosen.push((Item::Agent(a.name), true));
                }
            }
        }
    }
    match acp {
        None => {}
        Some("none") => {}
        Some("all") => chosen.extend(all.iter().filter(|i| matches!(i, Item::Jazyk(_))).map(|i| (i.clone(), true))),
        Some(v) => {
            for n in names(v) {
                match crate::setup::ACP_HOSTS.iter().find(|h| **h == n) {
                    Some(h) => chosen.push((Item::Jazyk(h), true)),
                    None => snippets.push(n),
                }
            }
        }
    }
    let rest: Vec<Item> = all
        .into_iter()
        .filter(|i| match i {
            Item::Jazyk(_) => acp.is_none(),
            Item::Agent(_) | Item::Unsupported(_) => mcp.is_none(),
        })
        .collect();
    if rest.iter().any(|i| i.enabled()) {
        if tty {
            println!();
            match crate::setup::choose(&rest, "Set up jazyk for which agents?", tty, false) {
                Some(picked) => chosen.extend(picked.into_iter().map(|i| (i, false))),
                None => println!("jazyk: skipping agent setup"),
            }
        } else {
            println!("jazyk: skipping agent setup (no interactive stdin); rerun with --mcp all --acp all");
        }
    }
    let mut wrote = false;
    for ide in snippets {
        wrote |= run_acp_install(Some(&ide)) == 0;
    }
    if chosen.is_empty() {
        return wrote;
    }
    crate::setup::warn_if_absolute();
    let mut done = Vec::new();
    let mut failed = Vec::new();
    for (item, named) in chosen {
        match crate::setup::install(&item, cwd, tty, named) {
            Ok(()) => done.push(item),
            Err(e) => failed.push((item, e)),
        }
    }
    wrote |= !done.is_empty();
    println!("jazyk: agent setup");
    crate::setup::print_summary(&done, &failed, "jazyk setup");
    wrote
}

pub fn run_gui(paths: &[String], opts: &Options) -> i32 {
    let (proj, llm, out) = resolve(paths, opts);
    crate::gui::run(
        proj,
        llm,
        out,
        crate::gui::GuiOptions {
            port: opts.port,
            no_open: opts.no_open,
            watch: opts.watch,
            gui_url: opts.gui_url.clone(),
            no_token: opts.no_token,
            dev: opts.dev,
            cli_opts: opts.clone(),
        },
    )
}

// Resolve the project (walking up to jazyk.toml, or ad hoc with explicit paths), the LLM
// (flag → env → project → global config → default), and the out directory.
pub fn resolve(paths: &[String], opts: &Options) -> (Project, Llm, PathBuf) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = project::find_root(&cwd).unwrap_or_else(|| cwd.clone());
    let mut proj = if let Some(set) = opts.set.as_deref() {
        Project::load_set(&root, set).unwrap_or_else(|(_, error)| Project { root: root.clone(),
            config_error: Some(error), docs_glob: vec![], ..Project::default() })
    } else { Project::load(&root) };
    let _ = paths; // Path filters choose sets in run_compile; never truncate a graph's input.

    let global = project::load_global_llm();
    let llm = resolve_llm(opts, &proj.llm, &global, |name| std::env::var(name).ok());

    let out = opts
        .out
        .clone()
        .map(|o| {
            let pb = PathBuf::from(o);
            if pb.is_absolute() {
                pb
            } else {
                cwd.join(pb)
            }
        })
        .unwrap_or_else(|| proj.out.clone());
    if opts.out.is_some() && (opts.set.is_none() || (out.starts_with(root.join(".jazyk")) && out!=proj.out)) {
        proj.config_error=Some("--out requires --set and cannot target project metadata or another set's store".into());
    }
    // The out directory is never doc input; doc_files skips it by path.
    proj.out = out.clone();
    (proj, llm, out)
}

// Per-field precedence: CLI flag → env → project [llm] → global config → built-in default.
// Mirrors docs/compiler/project-settings.md#llm. `env` is injected for testability.
pub(crate) fn resolve_llm(
    opts: &Options,
    proj: &project::LlmSettings,
    global: &project::GlobalLlm,
    env: impl Fn(&str) -> Option<String>,
) -> Llm {
    let base_url = opts
        .base_url
        .clone()
        .or_else(|| env("JAZYK_LLM_BASE_URL"))
        .or_else(|| proj.base_url.clone())
        .or_else(|| global.base_url.clone())
        .unwrap_or_else(|| "http://localhost:11434/v1".to_string());
    let model = opts
        .model
        .clone()
        .or_else(|| env("JAZYK_MODEL"))
        .or_else(|| proj.model.clone())
        .or_else(|| global.model.clone())
        .unwrap_or_else(|| "llama3.1".to_string());
    // The env tier reads the variable named by api_key_env (itself resolved
    // project → global → default), then literal keys follow the same order.
    let api_key_env = proj
        .api_key_env
        .clone()
        .or_else(|| global.api_key_env.clone())
        .unwrap_or_else(|| "JAZYK_API_KEY".to_string());
    let api_key = opts
        .api_key
        .clone()
        .or_else(|| env(&api_key_env))
        .or_else(|| proj.api_key.clone())
        .or_else(|| global.api_key.clone())
        .unwrap_or_default();
    let temperature = env("JAZYK_TEMPERATURE")
        .and_then(|s| s.parse::<f64>().ok())
        .or(proj.temperature)
        .or(global.temperature)
        .or(Some(0.0))
        .filter(|t| *t >= 0.0);
    // The trace is attached per run by whoever starts the work (`with_trace`).
    Llm {
        api: opts.llm_api.clone().or_else(|| env("JAZYK_LLM_API")).or_else(|| proj.api.clone()).or_else(|| global.api.clone()).unwrap_or_else(|| "chat_completions".into()),
        reasoning_summary: opts.reasoning_summary.clone().or_else(|| env("JAZYK_REASONING_SUMMARY")).or_else(|| proj.reasoning_summary.clone()).or_else(|| global.reasoning_summary.clone()),
        base_url,
        model,
        api_key,
        temperature,
        trace: None,
    }
}

// Choose the agent that performs AI work, and for the embedded agent the model it
// prompts. Both land in jazyk.toml, so the project carries its own answer and no
// later command has to ask again. `--agent` skips the prompt; a non-interactive
// stdin skips the step. Mirrors docs/frontends/cli.md#jazyk-init.
fn init_agent(cwd: &std::path::Path, flag: Option<&str>) -> bool {
    use std::io::IsTerminal;
    let path = cwd.join(crate::document_sets::CONFIG);
    let interactive = flag.is_none() && std::io::stdin().is_terminal();
    let name = match flag {
        Some("none") => return false,
        Some(a) => a.to_string(),
        None => {
            if !interactive {
                println!(
                    "jazyk: keeping the default agent (embedded); `jazyk init --agent {}` chooses another",
                    ACP_AGENTS.iter().map(|a| a.0).collect::<Vec<_>>().join("|")
                );
                return false;
            }
            println!("\nWhich agent should do the AI work? Jazyk drives it over ACP.");
            for (i, (_, label, cmd, args, _)) in ACP_AGENTS.iter().enumerate() {
                let how = if cmd.is_empty() {
                    String::new()
                } else {
                    format!("   ({} {})", cmd, args.join(" "))
                };
                println!("  {}) {}{}", i + 1, label, how);
            }
            match ask(&format!("choose [1-{}] (default 1): ", ACP_AGENTS.len()))
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|n| *n >= 1 && *n <= ACP_AGENTS.len())
            {
                Some(n) => ACP_AGENTS[n - 1].0.to_string(),
                None => crate::acp::config::EMBEDDED.to_string(),
            }
        }
    };
    let Some(agent) = ACP_AGENTS.iter().find(|a| a.0 == name) else {
        eprintln!(
            "jazyk: unknown agent `{}`; one of {}",
            name,
            ACP_AGENTS
                .iter()
                .map(|a| a.0)
                .collect::<Vec<_>>()
                .join(", ")
        );
        return false;
    };
    // A built-in agent resolves by name, so only the name is recorded.
    let old = crate::vfs::read_to_string(&path).unwrap_or_default();
    let mut full = crate::mcp::toml_set(&old, "acp", "agent", agent.0);
    if agent.0 == crate::acp::config::EMBEDDED && interactive {
        // The embedded agent prompts a model, so init asks which one. The endpoint is
        // whatever the config ladder resolves; asking it what it serves beats making
        // the user recall model names. Mirrors docs/frontends/acp.md#choosing-a-model.
        let (_, llm, _) = resolve(&[], &Options::default());
        println!("\nAsking {} what models it serves...", llm.base_url);
        let models = llm.list_models();
        let chosen = match models.len() {
            0 => {
                println!(
                    "jazyk: the endpoint did not answer; keeping model `{}` (change it in jazyk.toml or ~/.jazyk/config.toml)",
                    llm.model
                );
                String::new()
            }
            _ => {
                for (i, m) in models.iter().enumerate().take(20) {
                    let mark = if *m == llm.model { "  (current)" } else { "" };
                    println!("  {}) {}{}", i + 1, m, mark);
                }
                if models.len() > 20 {
                    println!("  ... and {} more; type a name instead", models.len() - 20);
                }
                let answer = ask(&format!(
                    "choose [1-{}], a model name, or blank to keep `{}`: ",
                    models.len().min(20),
                    llm.model
                ));
                let answer = answer.trim().to_string();
                match answer
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n >= 1 && *n <= models.len())
                {
                    Some(n) => models[n - 1].clone(),
                    None if answer.is_empty() => String::new(),
                    None => answer,
                }
            }
        };
        if !chosen.is_empty() {
            full = crate::mcp::toml_set(&full, "llm", "model", &chosen);
            println!("jazyk: model {}", chosen);
        }
    }
    if full == old {
        return false;
    }
    match crate::vfs::write(&path, &full) {
        Ok(()) => {
            println!("jazyk: agent {} recorded in {}", agent.0, path.display());
            true
        }
        Err(e) => {
            eprintln!("jazyk: cannot write {}: {}", path.display(), e);
            false
        }
    }
}

fn ask(prompt: &str) -> String {
    use std::io::Write;
    print!("{}", prompt);
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).ok();
    line
}

// Register the Jazyk entry in an editor's agent registry. No client supports
// per-project registration, so one global entry serves every project and the proxy
// resolves the project from the session's cwd instead.
// Mirrors docs/frontends/acp.md#registration.
pub fn run_acp_install(ide: Option<&str>) -> i32 {
    let Some(ide) = ide else {
        eprintln!(
            "usage: jazyk acp install --ide <{}> (or: jazyk acp install zed)",
            crate::acp::install::IDES.join("|")
        );
        return 2;
    };
    crate::acp::install::install(ide)
}

// Spawn the run's ACP runner (docs/frontends/acp.md#worker-sessions), or explain why not.
fn runner_for(
    proj: &Project,
    llm: &Llm,
    out: &std::path::Path,
) -> Result<crate::acp::runner::AcpRunner, String> {
    crate::acp::runner::AcpRunner::start(proj, llm, out)
}

fn trace_for(opts: &Options) -> Trace {
    Trace::stderr(if opts.quiet {
        TraceLevel::Quiet
    } else if opts.verbose {
        TraceLevel::Verbose
    } else {
        TraceLevel::Normal
    })
}

// The build's last line: the verdict with its counts, e.g. `converged, 2 blocked,
// 1 optional advised`. Mirrors docs/frontends/cli.md#jazyk-compile.
fn verdict_line(r: &reconcile::BuildReport) -> String {
    r.verdict.clone()
}

pub fn run_compile(paths: &[String], opts: &Options) -> i32 {
    use crate::document_sets::{Workspace, included};
    let cwd=std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root=project::find_root(&cwd).unwrap_or_else(|| cwd.clone());
    let workspace=match Workspace::load(&root) { Ok(w)=>w, Err((_,e))=>{eprintln!("jazyk: {}",e);return 2;} };
    if workspace.sets.is_empty() { eprintln!("jazyk: no document sets; use `jazyk sets add NAME --docs PATH`"); return 2; }
    let summary=workspace.summary();
    let mut code=0;
    let mut selected=0;
    for set in &workspace.sets {
        if let Some(selector)=opts.set.as_deref() {
            match workspace.select(selector) { Ok(s) if s.id == set.id => {}, Ok(_) => continue,
                Err((_,e)) => {eprintln!("jazyk: {}",e);return 2;} }
        } else if !paths.is_empty() {
            if !paths.iter().any(|path| cwd.join(path).strip_prefix(&root).ok()
                .is_some_and(|rel| included(&set.docs, &rel.to_string_lossy()) || workspace.files().iter()
                    .any(|f| f.starts_with(&format!("{}/",rel.display())) && included(&set.docs,f)))) {continue;}
        } else if !summary["sets"].as_array().unwrap().iter().any(|s| s["id"]==set.id && s["dirty"]==true && s["state"]!="empty") {continue;}
        selected+=1;
        let mut options=opts.clone(); options.set=Some(set.id.clone());
        let (proj,llm,out)=resolve(&[],&options);
        if let Some(error)=proj.config_error.as_deref() { eprintln!("jazyk: {}",error);return 2; }
        println!("{}",set.name);
        if opts.verbose {llm::set_verbose(true);}
        let trace=trace_for(opts).with_transcript(&out,"compile");
        let report=reconcile::compile_with(&proj,&llm,&out,&trace,opts.sessions);
        trace.finish_transcript("done",&serde_json::json!(report));
        println!("{}: {}",set.name,verdict_line(&report));
        if !report.converged() {code=1;}
    }
    if selected==0 { println!("No dirty document sets."); }
    code
}

pub fn run_sets(args: &[String], opts: &Options) -> i32 {
    let cwd=std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root=project::find_root(&cwd).unwrap_or(cwd);
    let result=(|| -> crate::document_sets::Result<serde_json::Value> {
        let workspace=crate::document_sets::Workspace::load(&root)?;
        let action=args.first().map(String::as_str).unwrap_or("list");
        if action=="list" { return Ok(workspace.summary()); }
        let name=args.get(1).ok_or_else(|| (400,"a set name or ID is required".into()))?;
        let mut set=serde_json::json!({});
        if action=="add" { set=serde_json::json!({"name":name,"docs":opts.docs,"roots":opts.roots,"source":opts.source}); }
        else {
            set["id"]=serde_json::json!(workspace.select(name)?.id);
            if let Some(name)=&opts.name {set["name"]=serde_json::json!(name);}
            if !opts.docs.is_empty() {set["docs"]=serde_json::json!(opts.docs);}
            if !opts.source.is_empty() {set["source"]=serde_json::json!(opts.source);}
            if !opts.roots.is_empty() {set["roots"]=serde_json::json!(opts.roots);}
        }
        if !["add","update","remove","preview"].contains(&action) {return Err((400,"use sets list|add|update|remove|preview".into()));}
        let body=if action=="remove" {serde_json::json!({"baseHash":workspace.hash,"remove":set["id"]})}
            else {serde_json::json!({"baseHash":workspace.hash,"set":set})};
        crate::document_sets::edit(&root,&body,action=="preview")
    })();
    match result {Ok(v)=>{println!("{}",serde_json::to_string_pretty(&v).unwrap());0},Err((_,e))=>{eprintln!("jazyk: {}",e);2}}
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
        println!("next: `jazyk compile` runs the compile goals (or an attached worker claims them)");
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
                println!("  fact: {} {} = {}", f["id"].as_str().unwrap_or_default(), f["field"].as_str().unwrap_or_default(), f["value"]);
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
        files.push(crate::gen::Ledger::path(&out));
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

// One watch-loop build: goal lines instead of the tool-row trace, unless --verbose
// asked for the whole thing. Mirrors docs/frontends/cli.md#jazyk-watch.
fn watch_build(paths: &[String], opts: &Options) -> i32 {
    if opts.verbose {
        return run_compile(paths, opts);
    }
    if opts.set.is_none() {return run_compile(paths,opts);}
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
        let Ok(w)=crate::document_sets::Workspace::load(&proj.root) else {return String::new();};
        let mut s = w.hash.clone();
        for f in w.files().iter().filter(|f|!w.memberships(f).is_empty()).map(|f|w.root.join(f)) {
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
    let incomplete =
        |out: &std::path::Path| -> bool { Store::load(out).status.verdict.state == "incomplete" };
    let mut backoff = std::time::Duration::from_secs(30);
    let max_backoff = std::time::Duration::from_secs(300);
    let mut last = fingerprint(&proj);
    watch_build(paths, opts);
    loop {
        let retry_due = if incomplete(&out) {
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

// status.yaml and the board: the store version, the last verdict with its counts,
// the live board counts, coverage, diagnostics, the last build's cost, and the
// unclaimed report. Mirrors docs/frontends/cli.md#jazyk-status.
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
            eprintln!("jazyk spend: `{}` is not a generation (g42 or 42)", generation);
            return 2;
        }
    };
    let tokens = match tokens.parse::<u64>() {
        Ok(t) if t > 0 => t,
        _ => {
            eprintln!("jazyk spend: tokens takes a whole number above zero, got `{}`", tokens);
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
fn spend_at(out: &Path, generation: u64, tokens: u64, cached: Option<u64>) -> Result<String, String> {
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
            println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
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
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    } else {
        print!("{}", render_status(&report));
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
        let verdict = set["verdict"]["state"].as_str().filter(|v| !v.is_empty()).unwrap_or("none");
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
    s.push_str(&format!("verdict: {}\n", r["verdict"].as_str().unwrap_or("")));
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
fn board_from_disk(
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
fn ripple_text_for(store: &Store, root: Option<&str>, back: bool) -> Result<String, String> {
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
        let (project,_,_)=resolve(&[],opts);
        if let Ok(w)=crate::document_sets::Workspace::load(&project.root) {
            if w.sets.len()!=1 {
                let mut code=0;
                for set in w.sets {let mut local=opts.clone();local.set=Some(set.id);println!("{}",set.name);code=code.max(run_query(&local,query));}
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{GlobalLlm, LlmSettings};

    #[test]
    fn llm_precedence_flag_env_project_global_default() {
        let no_env = |_: &str| None;
        let proj = LlmSettings {
            model: Some("proj-model".to_string()),
            temperature: Some(0.5),
            ..Default::default()
        };
        let global = GlobalLlm {
            base_url: Some("http://global".to_string()),
            model: Some("global-model".to_string()),
            temperature: Some(0.9),
            ..Default::default()
        };

        // Project beats global; unset project fields fall through to global.
        let r = resolve_llm(&Options::default(), &proj, &global, no_env);
        assert_eq!(r.model, "proj-model");
        assert_eq!(r.base_url, "http://global");
        assert_eq!(r.temperature, Some(0.5));

        // Env beats project, flag beats env.
        let env = |name: &str| (name == "JAZYK_MODEL").then(|| "env-model".to_string());
        let r = resolve_llm(&Options::default(), &proj, &global, env);
        assert_eq!(r.model, "env-model");
        let opts = Options {
            model: Some("flag-model".to_string()),
            ..Default::default()
        };
        let r = resolve_llm(&opts, &proj, &global, env);
        assert_eq!(r.model, "flag-model");

        // Nothing set anywhere: built-in defaults.
        let r = resolve_llm(
            &Options::default(),
            &LlmSettings::default(),
            &GlobalLlm::default(),
            no_env,
        );
        assert_eq!(r.model, "llama3.1");
        assert_eq!(r.base_url, "http://localhost:11434/v1");
        assert_eq!(r.temperature, Some(0.0));
    }

    // The help text names the goal commands and carries no em dash anywhere; the
    // top line says goal-based. Every command the top usage lists has a shape (its
    // arity and options) and a help page of its own, and each help page names only
    // options the parser accepts for it. Mirrors docs/frontends/cli.md#help.
    #[test]
    fn help_lists_the_goal_commands() {
        let usage = crate::launcher::top_usage();
        for cmd in ["preview", "explain", "ripple", "acp", "monitor", "release", "answer"] {
            assert!(usage.contains(cmd), "`{}` missing from the top usage", cmd);
        }
        assert!(usage.contains("goal-based"), "{}", usage);
        assert!(!usage.contains('\u{2014}'), "em dash in the top usage");
        // The groups come in the documented order: what a person types first,
        // what programs spawn last.
        let start = usage.find("start here:").expect("start here group");
        let ides = usage.find("run by IDEs and agents").expect("IDE group");
        assert!(start < ides, "{}", usage);
        for (line, _) in crate::launcher::COMMAND_GROUPS.iter().flat_map(|(_, cs)| cs.iter()) {
            let Some(cmd) = line.split_whitespace().next() else {
                continue; // bare `jazyk`, the launcher
            };
            if cmd == "<choice>" {
                // One line for every launcher choice; each is its own command.
                for c in crate::start::CHOICES.iter().filter(|c| c.id != "gui") {
                    assert!(crate::launcher::cmd_usage(c.id).is_some(), "no help for `{}`", c.id);
                }
                continue;
            }
            let (_, extra) = crate::launcher::command_shape(cmd).unwrap_or_else(|| panic!("no shape for `{}`", cmd));
            let u = crate::launcher::cmd_usage(cmd).unwrap_or_else(|| panic!("no help for `{}`", cmd));
            assert!(!u.contains('\u{2014}'), "em dash in `{}` usage", cmd);
            for opt in extra.iter() {
                assert!(
                    u.contains(opt),
                    "`{}` honors {} but its help does not mention it",
                    cmd,
                    opt
                );
            }
        }
    }

    // The aggregate status reads as one line per set, never as JSON.
    // Mirrors docs/frontends/cli.md#jazyk-status.
    #[test]
    fn status_without_a_set_is_one_line_per_set() {
        let summary = serde_json::json!({"sets": [
            {"name": "Documents", "state": "clean", "generation": 32, "verdict": {"state": ""},
             "counts": {"open": 0, "ready": 0, "blocked": 33, "parked": 0, "failed": 0}, "warnings": []},
            {"name": "Orders", "state": "dirty", "generation": 4, "verdict": {"state": "converged"},
             "counts": {"open": 2, "ready": 1}, "warnings": ["No files match x/**"]}
        ]});
        assert_eq!(
            render_sets_overview(&summary),
            "Documents: clean, generation 32, verdict none; 0 open, 0 ready, 33 blocked, 0 parked, 0 failed\n\
             Orders: dirty, generation 4, verdict converged; 2 open, 1 ready, 0 blocked, 0 parked, 0 failed\n  \
             warning: No files match x/**\n"
        );
        assert!(render_sets_overview(&serde_json::json!({"sets": []})).contains("jazyk sets add"));
    }

    // `jazyk spend` stamps an agent's report on a session entry that carries no figure
    // and refuses one that does, saying what it carries.
    // Mirrors docs/frontends/cli.md#jazyk-spend.
    #[test]
    fn spend_stamps_an_unmeasured_session_and_refuses_a_measured_one() {
        let out = std::env::temp_dir().join(format!("jazyk-cli-spend-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        crate::vfs::create_dir_all(out.join("journal")).unwrap();
        for (g, tokens) in [(7u64, None), (8, Some(9800u64))] {
            let entry = crate::model::JournalEntry {
                build: format!("g{}", g),
                generation: g,
                kind: "session".into(),
                tokens,
                ..Default::default()
            };
            crate::vfs::write(
                out.join("journal").join(format!("g{}.yaml", g)),
                serde_norway::to_string(&entry).unwrap(),
            )
            .unwrap();
        }
        assert_eq!(
            spend_at(&out, 7, 18400, Some(9100)).unwrap(),
            "g7: 18400 tokens, 9100 cached, reported by the agent"
        );
        let r = crate::costs::report_at(&out);
        assert_eq!(
            (r["measured"].clone(), r["reported"].clone(), r["tokens"].clone()),
            (serde_json::json!(1), serde_json::json!(1), serde_json::json!(28200))
        );
        let err = spend_at(&out, 8, 100, None).unwrap_err();
        assert!(err.contains("already carries 9800 tokens"), "{}", err);
        assert!(spend_at(&out, 7, 100, None).unwrap_err().contains("reported by the agent"));
        crate::vfs::remove_dir_all(&out).ok();
    }

    // A one-document project in a temp dir: the smallest board with a ready batch.
    fn temp_project(name: &str) -> (Project, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("jazyk-cli-{}-{}", name, crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
        crate::vfs::write(dir.join("docs/a.md"), "# A\n\nThe body.\n").unwrap();
        let project = Project::load(&dir);
        let out = project.out.clone();
        crate::vfs::create_dir_all(&out).unwrap();
        crate::control::Control {
            compile: "auto".into(),
            ..Default::default()
        }
        .save(&out);
        (project, out)
    }

    // The previewed PROTOCOL line names the batch the board minted (b<generation>-<n>,
    // numbered from 1), the same id `begin_goals` accepts: a preview formatting its
    // own id sent a real session's first call into `no-ready-batch`. The batch id and
    // a document path resolve to the same prompt. Mirrors docs/frontends/cli.md#jazyk-preview.
    #[test]
    fn preview_names_the_boards_own_batch() {
        let (proj, out) = temp_project("preview");
        let (store, control, board) = board_from_disk(&proj, &out);
        let batch = board.batches.first().expect("a ready batch").clone();
        assert!(!batch.id.ends_with("-0"), "batches number from 1: {}", batch.id);

        let lint = crate::project::Linting::default();
        let text = preview_text(&store, &control, &board, "", &lint).unwrap();
        let proto = text
            .lines()
            .find(|l| l.starts_with("PROTOCOL"))
            .expect("a protocol line");
        assert!(proto.contains(&batch.id), "{}", proto);
        assert!(!proto.contains(&format!("b{}-0", store.status.generation)), "{}", proto);
        // The mode is the control plane's, never a constant.
        assert!(text.contains("auto mode"), "{}", text);

        assert_eq!(
            preview_text(&store, &control, &board, &batch.id, &lint).unwrap(),
            text
        );
        let by_doc = preview_text(&store, &control, &board, "docs/a.md", &lint).unwrap();
        assert!(by_doc.contains(&batch.id), "{}", by_doc);
        assert!(preview_text(&store, &control, &board, "b0-99", &lint).is_err());
        assert!(preview_text(&store, &control, &board, "ent:ghost", &lint).is_err());
        crate::vfs::remove_dir_all(proj.root).ok();
    }

    // `jazyk status` closes with what to do next: on a fresh project the first build,
    // and the ready count with the command that runs it; the JSON carries the same
    // lines. Mirrors docs/frontends/cli.md#jazyk-status.
    #[test]
    fn status_says_what_to_do_next() {
        let (proj, out) = temp_project("status");
        let (store, _control, board) = board_from_disk(&proj, &out);
        let report = status_report(&proj, &store, &board);
        let next: Vec<&str> = report["next"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(next.iter().any(|n| n.contains("`jazyk compile` runs the first build")), "{:?}", next);
        assert!(next.iter().any(|n| n.contains("ready: `jazyk compile` runs them")), "{:?}", next);
        let text = render_status(&report);
        assert!(text.starts_with("store: version"), "{}", text);
        assert!(text.contains("\nnext: "), "{}", text);
        assert!(text.contains("shape: "), "{}", text);
        crate::vfs::remove_dir_all(proj.root).ok();
    }

    // The verdict line the build prints last carries the counts, in both shapes.
    // Mirrors docs/frontends/cli.md#jazyk-compile.
    #[test]
    fn the_verdict_line_renders_its_counts() {
        let v = crate::model::Verdict {
            state: "converged".into(),
            blocked: 2,
            optional: 1,
            ..Default::default()
        };
        let r = reconcile::BuildReport {
            verdict: v.to_string(),
            ..Default::default()
        };
        assert_eq!(verdict_line(&r), "converged, 2 blocked, 1 optional advised");

        let v = crate::model::Verdict {
            state: "incomplete".into(),
            open: 3,
            failed: 1,
            blocked: 2,
            optional: 5,
        };
        let r = reconcile::BuildReport {
            verdict: v.to_string(),
            ..Default::default()
        };
        assert_eq!(
            verdict_line(&r),
            "incomplete: 3 open, 1 failed, 2 blocked, 5 optional advised"
        );
    }

    // A three-generation journal (an edit, a session resolving its goal and opening
    // another, a session resolving that one) renders as one indented cascade, and a
    // bare `jazyk ripple` roots at the same place: the last build.
    // Mirrors docs/frontends/cli.md#jazyk-ripple.
    #[test]
    fn ripple_renders_a_three_generation_cascade() {
        use crate::model::{Cause, JournalEntry, OpenedGoal, Resolved};
        let dir = std::env::temp_dir().join(format!("jazyk-ripple-test-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("journal")).unwrap();
        let sec = "g:reconcile-section:docs/orders.md#/orders/holds";
        let pair = "g:rejudge-pair:req:orders-6~req:payment-9";
        let g1 = JournalEntry {
            build: "g1".into(),
            generation: 1,
            kind: "edit".into(),
            dirtied: vec!["docs/orders.md#/orders/holds".into()],
            opened_goals: vec![OpenedGoal {
                goal: sec.into(),
                cause: Cause {
                    generation: 1,
                    mutation: 0,
                    via: "section-dirty".into(),
                },
            }],
            ..Default::default()
        };
        let g2 = JournalEntry {
            build: "g2".into(),
            generation: 2,
            kind: "session".into(),
            batch: vec![sec.into()],
            resolved_goals: vec![Resolved {
                goal: sec.into(),
                justification: "req:orders-6 revised (quote, statement)".into(),
                evidence: serde_json::Value::Null,
            }],
            opened_goals: vec![OpenedGoal {
                goal: pair.into(),
                cause: Cause {
                    generation: 2,
                    mutation: 1,
                    via: "entities".into(),
                },
            }],
            tokens: Some(21_000),
            ..Default::default()
        };
        let g3 = JournalEntry {
            build: "g3".into(),
            generation: 3,
            kind: "session".into(),
            batch: vec![pair.into()],
            resolved_goals: vec![Resolved {
                goal: pair.into(),
                justification: "consistent".into(),
                evidence: serde_json::Value::Null,
            }],
            tokens: Some(8_000),
            ..Default::default()
        };
        for (n, e) in [(1, &g1), (2, &g2), (3, &g3)] {
            crate::vfs::write(
                dir.join("journal").join(format!("g{}.yaml", n)),
                serde_norway::to_string(e).unwrap(),
            )
            .unwrap();
        }
        let store = Store {
            out: dir.clone(),
            ..Default::default()
        };

        let text = ripple_text_for(&store, Some("g1"), false).unwrap();
        assert!(
            text.starts_with("edit docs/orders.md#/orders/holds (human) g1"),
            "{}",
            text
        );
        assert!(
            text.contains(
                "└─ reconcile-section docs/orders.md#/orders/holds g2: req:orders-6 revised"
            ),
            "{}",
            text
        );
        assert!(
            text.contains("└─ rejudge-pair req:orders-6~req:payment-9 g3: consistent"),
            "{}",
            text
        );
        assert!(text.contains("2 sessions"), "{}", text);
        assert!(text.contains("29k tokens"), "{}", text);
        assert!(text.contains("nothing parked or failed"), "{}", text);

        // No root names the last build, which is this whole cascade.
        assert_eq!(ripple_text_for(&store, None, false).unwrap(), text);
        // An unknown root is an error, not an empty tree.
        assert!(ripple_text_for(&store, Some("ent:ghost"), false).is_err());
        crate::vfs::remove_dir_all(&dir).ok();
    }

    // The shape line: nodes per depth, then the fan-out histogram in the bands the
    // registry values set. Mirrors docs/frontends/cli.md#jazyk-status.
    #[test]
    fn the_shape_line_renders_depths_and_the_fan_out_histogram() {
        use crate::limits::{CHILDREN_PER_ENTITY_HARD, CHILDREN_PER_ENTITY_SOFT};
        use crate::model::Entity;
        let (soft, hard) = (
            CHILDREN_PER_ENTITY_SOFT as usize,
            CHILDREN_PER_ENTITY_HARD as usize,
        );
        let empty = Store::default();
        assert_eq!(
            shape_line(&empty),
            format!(
                "shape: 0 nodes per depth; fan-out 2-{}: 0, {}-{}: 0, over {}: 0",
                soft,
                soft + 1,
                hard,
                hard
            )
        );

        // Three roots: one holds two children, one soft plus one, one hard plus one.
        let mut s = Store::default();
        let mut add = |id: &str, parent: Option<&str>| {
            s.graph.entities.insert(
                id.to_string(),
                Entity {
                    name: id.to_string(),
                    parent: parent.map(String::from),
                    ..Default::default()
                },
            );
        };
        for root in ["ent:a", "ent:b", "ent:c"] {
            add(root, None);
        }
        for (root, n) in [("ent:a", 2), ("ent:b", soft + 1), ("ent:c", hard + 1)] {
            for i in 0..n {
                add(&format!("{}-{}", root, i), Some(root));
            }
        }
        assert_eq!(
            shape_line(&s),
            format!(
                "shape: 3 / {} nodes per depth; fan-out 2-{}: 2, {}-{}: 1, over {}: 1",
                2 + soft + 1 + hard + 1,
                soft,
                soft + 1,
                hard,
                hard
            )
        );
        if (soft, hard) == (9, 15) {
            assert_eq!(
                shape_line(&s),
                "shape: 3 / 28 nodes per depth; fan-out 2-9: 2, 10-15: 1, over 15: 1"
            );
        }
    }

    #[test]
    fn api_key_env_name_resolves_project_first() {
        let proj = LlmSettings {
            api_key_env: Some("PROJ_KEY".to_string()),
            ..Default::default()
        };
        let global = GlobalLlm {
            api_key_env: Some("GLOBAL_KEY".to_string()),
            api_key: Some("global-literal".to_string()),
            ..Default::default()
        };
        let env = |name: &str| (name == "PROJ_KEY").then(|| "from-proj-env".to_string());
        let r = resolve_llm(&Options::default(), &proj, &global, env);
        assert_eq!(r.api_key, "from-proj-env");
        // With the env var unset, literal keys resolve project → global.
        let r = resolve_llm(&Options::default(), &proj, &global, |_| None);
        assert_eq!(r.api_key, "global-literal");
    }
}
