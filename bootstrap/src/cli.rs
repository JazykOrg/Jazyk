// CLI command implementations. Mirrors docs/frontends/cli.md.
use crate::context;
use crate::llm::{self, Llm};
use crate::project::{self, Project};
use crate::reconcile;
use crate::session::{Trace, TraceLevel};
use crate::store::Store;
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
    pub gui_dist: Option<String>,
    pub no_token: bool,
    pub mcp: Option<String>,
    pub json: bool,
    pub once: bool,
    // ACP: the agent profile (--agent), and the flags of bridge-spawned MCP servings
    // (docs/frontends/mcp.md#mcp-into-acp-sessions).
    pub agent: Option<String>,
    pub acp_ide: Option<String>,
    pub ephemeral: bool,
    pub only: Option<String>,
    pub build_token: Option<String>,
    pub serve_files: bool,
    pub edit_sink: Option<String>,
    pub packaged: bool,
    // `jazyk benchmark --project <dir> --goal <id>`: one goal's session from a copy
    // of a real project (docs/benchmark/benchmark.md#snippets-from-a-real-project).
    pub project: Option<String>,
    pub goal: Option<String>,
    // `jazyk compile --sessions N`: at most N sessions this run.
    pub sessions: Option<usize>,
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
            depth: None,
            expand: Vec::new(),
            back: false,
            force: false,
            kind: None,
            list: false,
            audit: false,
            port: None,
            no_open: false,
            watch: false,
            gui_dist: None,
            no_token: false,
            mcp: None,
            json: false,
            once: false,
            agent: None,
            acp_ide: None,
            ephemeral: false,
            only: None,
            build_token: None,
            serve_files: false,
            edit_sink: None,
            packaged: false,
            project: None,
            goal: None,
            sessions: None,
        }
    }
}

// Initialize the current directory as a project root: a minimal jazyk.toml plus
// optional MCP integration for a coding agent. Warns instead of overwriting.
// Mirrors docs/frontends/cli.md#jazyk-init.
pub fn run_init(opts: &Options) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = cwd.join("jazyk.toml");
    let mut wrote_something = false;
    if path.exists() {
        eprintln!(
            "jazyk: jazyk.toml already exists in {}; leaving it unchanged",
            cwd.display()
        );
    } else {
        let previous = project::find_root(&cwd);
        if let Err(e) = std::fs::write(&path, INIT_TOML) {
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
    match init_mcp(&cwd, opts.mcp.as_deref()) {
        Ok(true) => wrote_something = true,
        Ok(false) => {}
        Err(e) => {
            eprintln!("jazyk: {}", e);
            return 1;
        }
    }
    // Which agent does the AI work, and for the embedded one, which model.
    // Mirrors docs/frontends/cli.md#jazyk-init.
    if init_agent(&cwd, opts.agent.as_deref()) {
        wrote_something = true;
    }
    // ACP registration is global per editor; the proxy resolves the project from the
    // session's cwd. Mirrors docs/frontends/cli.md#jazyk-init.
    if init_acp(opts.acp_ide.as_deref()) {
        wrote_something = true;
    }
    if wrote_something {
        0
    } else {
        1
    }
}

// The starter jazyk.toml `jazyk init` (and the init_project chat tool) writes.
pub(crate) const INIT_TOML: &str =
    "# A directory with jazyk.toml is a Jazyk project. Globs resolve relative to it.\n\n\
[docs]\nglob = [\"docs/**/*.md\"]\n\n\
[roots]\nfiles = [\"docs/README.md\"]\n\n\
[gen]\ndeliverable = \"deliverable\"\n";

// Scaffold the layout the fresh jazyk.toml names: docs/ with a placeholder root
// document, and the deliverable directory. Existing directories and files stay
// untouched. Returns what it created and prints nothing: the same scaffold runs
// inside the MCP serving and the ACP proxy, where stdout carries the protocol and a
// stray line of prose corrupts the stream.
pub(crate) fn init_scaffold(cwd: &std::path::Path) -> Result<Vec<String>, String> {
    let mut made = Vec::new();
    for dir in ["docs", "deliverable"] {
        let path = cwd.join(dir);
        if !path.exists() {
            std::fs::create_dir(&path)
                .map_err(|e| format!("cannot create {}: {}", path.display(), e))?;
            made.push(format!("{}/", dir));
        }
    }
    let readme = cwd.join("docs/README.md");
    if !readme.exists() {
        let content = "# TODO: name the project\n\n\
                       TODO: describe what this project is and what the deliverable should be. This\n\
                       file is the root document: the compiler reads it first, and every other\n\
                       document under `docs/` should be reachable from it.\n";
        std::fs::write(&readme, content)
            .map_err(|e| format!("cannot write {}: {}", readme.display(), e))?;
        made.push("docs/README.md".to_string());
    }
    Ok(made)
}

// The agents init knows how to wire: display name, config path, and the JSON key that
// holds the server map.
const MCP_AGENTS: &[(&str, &str, &str, &str)] = &[
    ("claude", "Claude Code", ".mcp.json", "mcpServers"),
    ("cursor", "Cursor", ".cursor/mcp.json", "mcpServers"),
    ("vscode", "VS Code", ".vscode/mcp.json", "servers"),
    (
        "gemini",
        "Gemini CLI",
        ".gemini/settings.json",
        "mcpServers",
    ),
];

// Offer MCP integration: `--mcp` skips the prompt, a non-interactive stdin skips the
// whole step so scripts never hang. Existing files are merged, never overwritten.
fn init_mcp(cwd: &std::path::Path, flag: Option<&str>) -> Result<bool, String> {
    use std::io::IsTerminal;
    let choice = match flag {
        Some("none") => return Ok(false),
        Some(a) => a.to_string(),
        None => {
            if !std::io::stdin().is_terminal() {
                println!("jazyk: skipping MCP setup (no interactive stdin); rerun with --mcp claude|cursor|vscode|gemini");
                return Ok(false);
            }
            println!(
                "\nSet up MCP integration? The agent gets the graph tools over `jazyk mcp graph`."
            );
            println!("  1) none");
            for (i, (_, name, file, _)) in MCP_AGENTS.iter().enumerate() {
                println!("  {}) {:<12} ({})", i + 2, name, file);
            }
            print!("choose [1-{}] (default 1): ", MCP_AGENTS.len() + 1);
            use std::io::Write;
            std::io::stdout().flush().ok();
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).ok();
            match line.trim().parse::<usize>() {
                Ok(n) if (2..=MCP_AGENTS.len() + 1).contains(&n) => MCP_AGENTS[n - 2].0.to_string(),
                _ => return Ok(false),
            }
        }
    };
    let Some((_, name, rel, key)) = MCP_AGENTS.iter().find(|(id, ..)| *id == choice) else {
        return Err(format!(
            "unknown MCP agent `{}`; one of claude, cursor, vscode, gemini, none",
            choice
        ));
    };
    let path = cwd.join(rel);
    let mut root: serde_json::Value = match std::fs::read_to_string(&path) {
        Ok(text) => {
            serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON: {}", rel, e))?
        }
        Err(_) => serde_json::json!({}),
    };
    if !root.is_object() {
        return Err(format!("{} does not hold a JSON object", rel));
    }
    let servers = root
        .as_object_mut()
        .unwrap()
        .entry(key.to_string())
        .or_insert_with(|| serde_json::json!({}));
    if !servers.is_object() {
        return Err(format!("{}.{} does not hold a JSON object", rel, key));
    }
    if servers.get("jazyk").is_some() {
        eprintln!(
            "jazyk: {} already has a `jazyk` server entry; leaving it unchanged",
            rel
        );
        return Ok(false);
    }
    // Read-only by default; adding --write to args hands the agent the write tools.
    let mut entry = serde_json::json!({ "command": "jazyk", "args": ["mcp", "graph"] });
    if *key == "servers" {
        entry["type"] = serde_json::json!("stdio"); // the VS Code shape names the transport
    }
    servers["jazyk"] = entry;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {}", parent.display(), e))?;
    }
    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    std::fs::write(&path, text + "\n").map_err(|e| format!("cannot write {}: {}", rel, e))?;
    println!(
        "jazyk: wrote a read-only `jazyk` MCP server into {} for {}",
        rel, name
    );
    Ok(true)
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
            gui_dist: opts.gui_dist.clone(),
            no_token: opts.no_token,
            cli_opts: opts.clone(),
        },
    )
}

// Resolve the project (walking up to jazyk.toml, or ad hoc with explicit paths), the LLM
// (flag → env → project → global config → default), and the out directory.
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
        .unwrap_or_else(|| proj.root.join("jazyk-out"));
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
        base_url,
        model,
        api_key,
        temperature,
        trace: None,
    }
}

// The agents init can wire without being told anything else: the built-in one, and
// the external ones whose command line is public and stable.
// Mirrors docs/frontends/acp.md#agents.
pub const ACP_AGENTS: &[(&str, &str, &str, &[&str])] = &[
    (
        crate::acp::config::EMBEDDED,
        "Embedded (jazyk's own agent, over your LLM endpoint)",
        "",
        &[],
    ),
    (
        "codex",
        "Codex",
        "npx",
        &["--yes", "@zed-industries/codex-acp"],
    ),
    (
        "claude",
        "Claude Code",
        "npx",
        &["--yes", "@zed-industries/claude-code-acp"],
    ),
    ("opencode", "OpenCode", "opencode", &["acp"]),
];

// Choose the agent that performs AI work, and for the embedded agent the model it
// prompts. Both land in jazyk.toml, so the project carries its own answer and no
// later command has to ask again. `--agent` skips the prompt; a non-interactive
// stdin skips the step. Mirrors docs/frontends/cli.md#jazyk-init.
fn init_agent(cwd: &std::path::Path, flag: Option<&str>) -> bool {
    use std::io::IsTerminal;
    let path = cwd.join("jazyk.toml");
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
            for (i, (_, label, cmd, args)) in ACP_AGENTS.iter().enumerate() {
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
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let mut full = crate::mcp::toml_set(&old, "acp", "agent", agent.0);
    // An external agent needs its command line recorded; the embedded one is built in.
    if !agent.2.is_empty() {
        let section = format!("acp.agents.{}", agent.0);
        full = crate::mcp::toml_set(&full, &section, "command", agent.2);
        let args = agent
            .3
            .iter()
            .map(|a| format!("\"{}\"", a))
            .collect::<Vec<_>>()
            .join(", ");
        full = toml_set_raw(&full, &section, "args", &format!("[{}]", args));
    } else if interactive {
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
    match std::fs::write(&path, &full) {
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

// A TOML value that is not a string (an array, a number): same minimal edit, no
// quoting. Kept next to its caller because nothing else needs it.
fn toml_set_raw(text: &str, section: &str, key: &str, raw: &str) -> String {
    let quoted = crate::mcp::toml_set(text, section, key, "\u{0}");
    quoted.replace(
        &format!("{} = \"\u{0}\"", key),
        &format!("{} = {}", key, raw),
    )
}

fn ask(prompt: &str) -> String {
    use std::io::Write;
    print!("{}", prompt);
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).ok();
    line
}

// Offer ACP registration during init: `--acp` skips the prompt, a non-interactive
// stdin skips the step. Mirrors docs/frontends/cli.md#jazyk-init.
fn init_acp(flag: Option<&str>) -> bool {
    use std::io::IsTerminal;
    let ides = crate::acp::install::IDES;
    let choice = match flag {
        Some("none") => return false,
        Some(ide) => ide.to_string(),
        None => {
            if !std::io::stdin().is_terminal() {
                println!(
                    "jazyk: skipping ACP setup (no interactive stdin); rerun with --acp {}",
                    ides.join("|")
                );
                return false;
            }
            println!("\nRegister Jazyk as an ACP agent in an editor? The proxy activates only inside jazyk projects.");
            println!("  1) none");
            let cmd = crate::acp::install::spawn_command();
            for (i, id) in ides.iter().enumerate() {
                let label = crate::acp::install::ide(id, &cmd)
                    .map(|i| i.label)
                    .unwrap_or(id);
                println!("  {}) {}", i + 2, label);
            }
            print!("choose [1-{}] (default 1): ", ides.len() + 1);
            use std::io::Write;
            std::io::stdout().flush().ok();
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).ok();
            match line
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|n| *n >= 2 && *n <= ides.len() + 1)
            {
                Some(n) => ides[n - 2].to_string(),
                None => return false,
            }
        }
    };
    run_acp_install(Some(&choice)) == 0
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
    let (proj, llm, out) = resolve(paths, opts);
    if opts.verbose {
        llm::set_verbose(true);
    }
    let trace = trace_for(opts).with_transcript(&out, "compile");
    let report = reconcile::compile_with(&proj, &llm, &out, &trace, opts.sessions);
    trace.finish_transcript("done", &serde_json::json!(report));
    println!("{}", verdict_line(&report));
    if report.converged() {
        0
    } else {
        1
    }
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
    crate::control::release(&proj, &out, stage);
    let board = crate::board::Board::compute(&proj, &out);
    println!(
        "jazyk: released {}: {} compile, {} bind, {} generate goal(s) now ready",
        stage.unwrap_or("compile and generate"),
        board.ready_of(&crate::board::Board::graph_kinds()),
        board.ready_of(&["bind"]),
        board.ready_of(&["generate"]),
    );
    0
}

// The external agent's trigger: watch the same surfaces `watch` does, perform nothing,
// print the ready work and which MCP tool begins it on every state change. One block
// per notice, quiet otherwise; --json prints one object per line.
// Mirrors docs/frontends/cli.md#jazyk-monitor.
pub fn run_monitor(paths: &[String], opts: &Options) -> i32 {
    let (proj, _llm, out) = resolve(paths, opts);
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
        let rendered = if json_mode {
            board.answer().to_string()
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
                for g in board
                    .goals
                    .iter()
                    .filter(|g| matches!(g.state, crate::model::GoalState::Blocked { .. }))
                    .take(3)
                {
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
pub fn run_status(paths: &[String], opts: &Options) -> i32 {
    let (proj, _llm, out) = resolve(paths, opts);
    let store = Store::load(&out);
    let s = &store.status;
    println!("store: version {}, generation {}", s.version, s.generation);
    println!("verdict: {}", s.verdict);
    // The board is derived from disk the same way compile derives it, so on a tree
    // with pending edits this shows the goals the next build will run.
    let board = crate::board::Board::compute(&proj, &out);
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
    println!(
        "board: {} open ({} compile, {} gc), {} blocked, {} parked, {} failed, {} optional",
        counts.open,
        open_of("compile"),
        open_of("gc"),
        counts.blocked,
        counts.parked,
        counts.failed,
        counts.optional
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
    let pct = if total == 0 {
        100
    } else {
        covered * 100 / total
    };
    println!("coverage: {}% ({} of {} sections)", pct, covered, total);
    let mut by_sev: std::collections::BTreeMap<&str, usize> = Default::default();
    for d in store.graph.diagnostics.values() {
        if d.lifecycle == "open" && d.triage.as_deref() != Some("suppressed") {
            *by_sev.entry(d.severity.as_str()).or_default() += 1;
        }
    }
    println!(
        "diagnostics: {}",
        if by_sev.is_empty() {
            "none".to_string()
        } else {
            by_sev
                .iter()
                .map(|(k, v)| format!("{} {}{}", v, k, if *v == 1 { "" } else { "s" }))
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!("{}", shape_line(&store));
    // The last build's cost, with the biggest goal kind's share beside the totals.
    let c = &s.costs;
    if c.sessions > 0 || c.tokens > 0 {
        let tokens = if c.tokens >= 1000 {
            format!("{}k", c.tokens / 1000)
        } else {
            c.tokens.to_string()
        };
        let mut line = format!("cost: {} sessions, {} tokens", c.sessions, tokens);
        if let Some((kind, l)) = c.by_kind.iter().max_by_key(|(_, l)| (l.tokens, l.sessions)) {
            let share = if c.tokens > 0 {
                l.tokens * 100 / c.tokens
            } else {
                l.sessions * 100 / c.sessions.max(1)
            };
            if share > 0 {
                line.push_str(&format!(" ({}% {})", share, kind));
            }
        }
        println!("{}", line);
    }
    // The unattached remainder, summed over the ledger: generated mass no
    // requirement claims. Mirrors docs/consumers/gen.md#the-unattached-remainder.
    let ledger = crate::gen::Ledger::load(&out);
    let measured: Vec<(&String, &crate::gen::Unattached)> = ledger
        .entities
        .iter()
        .filter_map(|(slug, e)| e.unattached.as_ref().map(|u| (slug, u)))
        .collect();
    let (ufiles, ulines) = measured
        .iter()
        .fold((0u64, 0u64), |(f, l), (_, u)| (f + u.files, l + u.lines));
    if ufiles > 0 || ulines > 0 {
        let worst = measured
            .iter()
            .max_by(|a, b| {
                a.1.ratio
                    .partial_cmp(&b.1.ratio)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap();
        println!(
            "unattached: {} file(s), {} line(s) (worst {} at {:.2})",
            ufiles, ulines, worst.0, worst.1.ratio
        );
    }
    // The unclaimed report: deliverable files no binding names, the decompilation
    // worklist. Mirrors docs/consumers/bind.md#the-unclaimed-report.
    let gs = crate::gen::GenSettings::resolve(&proj);
    let un = crate::bind::unclaimed(&proj, &store, &gs);
    if !un.is_empty() {
        println!(
            "unclaimed: {} file(s) no binding names (`jazyk decompile` drafts docs for them)",
            un.len()
        );
        for f in un.iter().take(8) {
            println!("  - {}", f);
        }
        if un.len() > 8 {
            println!("  ... and {} more", un.len() - 8);
        }
    }
    // The medium warning: the ledger's toolchain and the recorded run commands
    // disagree. Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
    if let Some(w) = crate::gen::medium_divergence(&crate::gen::Ledger::load(&store.out)) {
        println!("medium: {}", w);
    }
    0
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
    let mut store = Store::load(&out);
    let (parsed, _) = reconcile::parse_all(&proj);
    store.sync_docs(&parsed);
    let control = crate::control::Control::load(&proj, &out);
    let board = crate::board::Board::derive(&store, &proj, &control);
    let target = args.first().map(String::as_str).unwrap_or("").trim();

    if target.is_empty() {
        // The batch the scheduler would claim next.
        let Some(batch) = board.batches.first() else {
            if board.open_goals().is_empty() {
                eprintln!(
                    "jazyk: nothing to preview; the board is empty ({})",
                    board.verdict()
                );
            } else {
                eprintln!(
                    "jazyk: no ready batch; `jazyk explain` says what every open goal waits for"
                );
            }
            return 1;
        };
        let goals = batch_goals(&board, batch);
        print!("{}", crate::session::preview(&store, &goals));
        return 0;
    }

    // A goal id, a batch id, or a target with goals on it.
    let goal = board.goal(target).cloned().or_else(|| {
        let on_target: Vec<&crate::model::Goal> =
            board.goals.iter().filter(|g| g.target == target).collect();
        on_target
            .iter()
            .find(|g| board.is_ready(&g.id))
            .or_else(|| on_target.first())
            .map(|g| (*g).clone())
    });
    if goal.is_none() {
        if let Some(batch) = board.batches.iter().find(|b| b.id == target) {
            let goals = batch_goals(&board, batch);
            print!("{}", crate::session::preview(&store, &goals));
            return 0;
        }
    }
    let Some(goal) = goal else {
        eprintln!(
            "jazyk: no goal on `{}`; `jazyk explain` lists the board",
            target
        );
        return 1;
    };
    // ratify and answer have no session: print the human path instead of a prompt.
    if crate::goals::blocked_on_human(&goal.kind) {
        println!("{} is blocked on a human; no session runs for it", goal.id);
        println!("  owed: {}", crate::session::gate_line(&goal.kind));
        println!("  change: {}", crate::session::change_line(&goal));
        for h in &goal.hints {
            println!("  hint: {}", h);
        }
        return 1;
    }
    // The batch the goal joins; a goal in no scheduled batch renders alone, behind
    // the readiness reason.
    if let Some(batch) = board
        .batches
        .iter()
        .find(|b| b.goals.iter().any(|id| id == &goal.id))
    {
        let goals = batch_goals(&board, batch);
        print!("{}", crate::session::preview(&store, &goals));
        return 0;
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
    println!("not ready: {}", reason);
    print!("{}", crate::session::preview(&store, &[goal]));
    0
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
    let mut store = Store::load(&out);
    let (parsed, _) = reconcile::parse_all(&proj);
    store.sync_docs(&parsed);
    let control = crate::control::Control::load(&proj, &out);
    let board = crate::board::Board::derive(&store, &proj, &control);
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
            eprintln!("jazyk: `{}` names no goal and no known target", target);
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
pub fn run_context(paths: &[String], opts: &Options, target: &str) -> i32 {
    let (_proj, _llm, out) = resolve(paths, opts);
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
    runner.set_build_token(Some(format!("internal-{}", std::process::id())));
    let store = Store::load(&out);
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
    // Binding first: owed bind tasks classify each requirement before any entity
    // regenerates, and the bound tests become generation's acceptance gates.
    // Mirrors docs/consumers/bind.md#generation-makes-bound-tests-pass.
    match crate::bind::run_all(&store, &runner, &gs, entities, &trace) {
        Ok(b) => {
            let (bound, bfail) = (
                b["bound"].as_u64().unwrap_or(0),
                b["failures"].as_u64().unwrap_or(0),
            );
            if bound + bfail > 0 {
                println!(
                    "jazyk: bind: {} bind goal(s) resolved, {} failure(s)",
                    bound, bfail
                );
            }
        }
        Err(e) => {
            eprintln!("jazyk: bind: {}", e);
        }
    }
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
            println!(
                "{:24} {:60} {}",
                rid,
                llm::truncate(&statement, 58),
                r["status"].as_str().unwrap_or("")
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
        let mut opts = Options::default();
        opts.model = Some("flag-model".to_string());
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
    // top line says goal-based. Mirrors docs/frontends/cli.md#help.
    #[test]
    fn help_lists_the_goal_commands() {
        let usage = crate::top_usage();
        for cmd in ["preview", "explain", "ripple"] {
            assert!(usage.contains(cmd), "`{}` missing from the top usage", cmd);
        }
        assert!(usage.contains("goal-based"), "{}", usage);
        assert!(!usage.contains('\u{2014}'), "em dash in the top usage");
        for cmd in [
            "compile", "check", "watch", "status", "preview", "explain", "ripple", "context",
            "monitor", "release", "gen", "test", "mcp", "docsgen",
        ] {
            let u = crate::cmd_usage(cmd).expect(cmd);
            assert!(!u.contains('\u{2014}'), "em dash in `{}` usage", cmd);
        }
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
        let dir = std::env::temp_dir().join(format!("jazyk-ripple-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("journal")).unwrap();
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
            tokens: 21_000,
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
            tokens: 8_000,
            ..Default::default()
        };
        for (n, e) in [(1, &g1), (2, &g2), (3, &g3)] {
            std::fs::write(
                dir.join("journal").join(format!("g{}.yaml", n)),
                serde_norway::to_string(e).unwrap(),
            )
            .unwrap();
        }
        let mut store = Store::default();
        store.out = dir.clone();

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
        std::fs::remove_dir_all(&dir).ok();
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
