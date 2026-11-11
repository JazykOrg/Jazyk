// Project setup: `init` (the scaffold, agent setup, the agent choice), `sets`, `gui`,
// and the ACP registration. Mirrors docs/frontends/cli.md#jazyk-init.
use super::{resolve, Options};
use crate::acp::config::BUILTIN_AGENTS as ACP_AGENTS;
use crate::project;
use crate::vfs::VPathExt;
use std::path::PathBuf;

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
        if let Err((_, e)) =
            crate::document_sets::atomic_config(&cwd, INIT_TOML, &crate::model::hash_hex(""))
        {
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
        list_known(
            m,
            &crate::setup::AGENTS
                .iter()
                .map(|a| a.name)
                .collect::<Vec<_>>(),
            "agent",
        )?;
    }
    if let Some(a) = opts.agent.as_deref() {
        if a != "none" && !ACP_AGENTS.iter().any(|x| x.0 == a) {
            return Err(format!(
                "unknown agent `{}`; one of {}, none",
                a,
                ACP_AGENTS
                    .iter()
                    .map(|x| x.0)
                    .collect::<Vec<_>>()
                    .join(", ")
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
            return Err(format!(
                "unknown {} `{}`; one of {}, all, none",
                what,
                name,
                known.join(", ")
            ));
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
    crate::document_sets::register_merge_driver(cwd);
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
    let names = |v: &str| {
        v.split(',')
            .map(|n| n.trim().to_string())
            .collect::<Vec<_>>()
    };
    match mcp {
        None => {}
        Some("none") => {}
        Some("all") => chosen.extend(
            all.iter()
                .filter(|i| matches!(i, Item::Agent(_)))
                .map(|i| (i.clone(), false)),
        ),
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
        Some("all") => chosen.extend(
            all.iter()
                .filter(|i| matches!(i, Item::Jazyk(_)))
                .map(|i| (i.clone(), true)),
        ),
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

pub fn run_sets(args: &[String], opts: &Options) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = project::find_root(&cwd).unwrap_or(cwd);
    let result = (|| -> crate::document_sets::Result<serde_json::Value> {
        let workspace = crate::document_sets::Workspace::load(&root)?;
        let action = args.first().map(String::as_str).unwrap_or("list");
        if action == "list" {
            return Ok(workspace.summary());
        }
        let name = args
            .get(1)
            .ok_or_else(|| (400, "a set name or ID is required".into()))?;
        let mut set = serde_json::json!({});
        if action == "add" {
            set = serde_json::json!({"name":name,"docs":opts.docs,"roots":opts.roots,"source":opts.source});
        } else {
            set["id"] = serde_json::json!(workspace.select(name)?.id);
            if let Some(name) = &opts.name {
                set["name"] = serde_json::json!(name);
            }
            if !opts.docs.is_empty() {
                set["docs"] = serde_json::json!(opts.docs);
            }
            if !opts.source.is_empty() {
                set["source"] = serde_json::json!(opts.source);
            }
            if !opts.roots.is_empty() {
                set["roots"] = serde_json::json!(opts.roots);
            }
        }
        if !["add", "update", "remove", "preview"].contains(&action) {
            return Err((400, "use sets list|add|update|remove|preview".into()));
        }
        let body = if action == "remove" {
            serde_json::json!({"baseHash":workspace.hash,"remove":set["id"]})
        } else {
            serde_json::json!({"baseHash":workspace.hash,"set":set})
        };
        crate::document_sets::edit(&root, &body, action == "preview")
    })();
    match result {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
            0
        }
        Err((_, e)) => {
            eprintln!("jazyk: {}", e);
            2
        }
    }
}
