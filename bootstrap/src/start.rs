use crate::vfs::VPathExt;
// The launcher: bare `jazyk` lists every way to work on the project, and each short
// name is a command that sets that way up and starts it.
// Mirrors docs/frontends/cli.md#jazyk.
use crate::cli::Options;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Web,
    Ide,
    Agent,
}

pub struct Choice {
    pub id: &'static str,
    pub kind: Kind,
    pub name: &'static str,
    // Where to get the program when it cannot be found.
    pub url: &'static str,
}

const fn c(id: &'static str, kind: Kind, name: &'static str, url: &'static str) -> Choice {
    Choice {
        id,
        kind,
        name,
        url,
    }
}

// Every choice, in menu order. Mirrors docs/frontends/cli.md#jazyk.
pub const CHOICES: &[Choice] = &[
    c("gui", Kind::Web, "Jazyk Web", ""),
    c(
        "vscode",
        Kind::Ide,
        "Visual Studio Code",
        "https://code.visualstudio.com",
    ),
    c("cursor", Kind::Ide, "Cursor", "https://cursor.com"),
    c("zed", Kind::Ide, "Zed", "https://zed.dev"),
    c(
        "idea",
        Kind::Ide,
        "IntelliJ IDEA",
        "https://www.jetbrains.com/idea",
    ),
    c(
        "phpstorm",
        Kind::Ide,
        "PhpStorm",
        "https://www.jetbrains.com/phpstorm",
    ),
    c(
        "webstorm",
        Kind::Ide,
        "WebStorm",
        "https://www.jetbrains.com/webstorm",
    ),
    c(
        "pycharm",
        Kind::Ide,
        "PyCharm",
        "https://www.jetbrains.com/pycharm",
    ),
    c(
        "goland",
        Kind::Ide,
        "GoLand",
        "https://www.jetbrains.com/go",
    ),
    c(
        "rustrover",
        Kind::Ide,
        "RustRover",
        "https://www.jetbrains.com/rust",
    ),
    c(
        "clion",
        Kind::Ide,
        "CLion",
        "https://www.jetbrains.com/clion",
    ),
    c(
        "rider",
        Kind::Ide,
        "Rider",
        "https://www.jetbrains.com/rider",
    ),
    c(
        "rubymine",
        Kind::Ide,
        "RubyMine",
        "https://www.jetbrains.com/ruby",
    ),
    c("paseo", Kind::Ide, "Paseo", "https://paseo.sh"),
    c(
        "claude",
        Kind::Agent,
        "Claude Code",
        "https://claude.com/claude-code",
    ),
    c(
        "codex",
        Kind::Agent,
        "Codex",
        "https://github.com/openai/codex",
    ),
    c(
        "gemini",
        Kind::Agent,
        "Gemini CLI",
        "https://github.com/google-gemini/gemini-cli",
    ),
    c("opencode", Kind::Agent, "OpenCode", "https://opencode.ai"),
    c("pi", Kind::Agent, "Pi", "https://pi.dev"),
    c("droid", Kind::Agent, "Factory Droid", "https://factory.ai"),
];

const GROUPS: [(Kind, &str); 2] = [(Kind::Ide, "IDEs"), (Kind::Agent, "Coding agents")];

// The JetBrains IDEs: Toolbox script name, and the application bundle name prefixes
// a macOS install may carry.
const JETBRAINS: &[(&str, &[&str])] = &[
    ("idea", &["IntelliJ IDEA"]),
    ("phpstorm", &["PhpStorm"]),
    ("webstorm", &["WebStorm"]),
    ("pycharm", &["PyCharm"]),
    ("goland", &["GoLand"]),
    ("rustrover", &["RustRover"]),
    ("clion", &["CLion"]),
    ("rider", &["Rider", "JetBrains Rider"]),
    ("rubymine", &["RubyMine"]),
];

// The choices `jazyk init --mcp` offers: everything whose setup is an MCP entry.
pub const MCP_CHOICES: &[&str] = &[
    "claude", "codex", "gemini", "opencode", "pi", "droid", "cursor",
];

pub fn choice(id: &str) -> Option<&'static Choice> {
    let id = if id == "code" { "vscode" } else { id };
    CHOICES.iter().find(|c| c.id == id)
}

fn interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

// Bare `jazyk` (None) or one choice by name. Mirrors docs/frontends/cli.md#jazyk.
pub fn run(opts: &Options, picked: Option<&str>) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let found = crate::project::find_root(&cwd);
    let tty = interactive();
    println!("Choose again: jazyk --setup");
    let mut prefs = match Preferences::load(found.as_deref().unwrap_or(&cwd)) {
        Ok(prefs) => prefs,
        Err(e) => {
            eprintln!("jazyk: {e}");
            return 1;
        }
    };
    let id = match picked {
        Some(id) => id.to_string(),
        None => {
            if found.is_some() {
                crate::cli::run_status(opts);
                println!();
            }
            if !tty {
                print!("{}", catalog());
                return 0;
            }
            if !opts.launcher_setup && !prefs.target.is_empty() {
                prefs.target.clone()
            } else {
                let groups = menu_groups();
                let Some((g, r)) = crate::menu::pick_preview(&groups, (0, 0), &target_preview)
                else {
                    return 0;
                };
                groups[g].rows[r].key.clone()
            }
        }
    };
    let Some(choice) = choice(&id) else {
        eprintln!("jazyk: unknown choice `{id}`; run jazyk --setup");
        return 2;
    };
    let root = match (found, choice.kind) {
        (Some(root), _) => root,
        (None, Kind::Web) => cwd.clone(),
        (None, _) => match init_here(&cwd) {
            Ok(()) => cwd.clone(),
            Err(e) => {
                eprintln!("jazyk: {e}");
                return 1;
            }
        },
    };
    let agent = if choice.kind == Kind::Ide {
        match select_agent(choice, &root, opts, tty, &prefs) {
            Ok(Some(agent)) => Some(agent),
            Ok(None) => return 0,
            Err(e) => {
                eprintln!("jazyk: {e}");
                return 1;
            }
        }
    } else {
        None
    };
    if let Err(e) = setup(choice, &root, opts, tty).and_then(|_| {
        agent
            .as_deref()
            .map_or(Ok(()), |agent| configure_ide(choice, &root, agent))
    }) {
        eprintln!("jazyk: {e}");
        return 1;
    }
    // Setup succeeds before preferences are saved, including failures in the IDE installer.
    if program(choice.id, &root).is_some() && root.join(crate::document_sets::CONFIG).is_file() {
        prefs.target = choice.id.to_string();
        if let Some(agent) = &agent {
            prefs.agents.insert(choice.id.to_string(), agent.clone());
        }
        if let Err(e) = prefs.save(&root) {
            eprintln!("jazyk: {e}");
            return 1;
        }
    }
    start(choice, &root)
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct Preferences {
    #[serde(default)]
    target: String,
    #[serde(default)]
    agents: std::collections::BTreeMap<String, String>,
}
impl Preferences {
    fn load(root: &Path) -> Result<Self, String> {
        let path = root.join(".jazyk/local/launcher.toml");
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }
    fn save(&self, root: &Path) -> Result<(), String> {
        crate::editor_lsp::write_atomic(
            &root.join(".jazyk/local/launcher.toml"),
            toml::to_string(self).map_err(|e| e.to_string())?.as_bytes(),
        )
        .map(|_| ())
    }
}

fn select_agent(
    choice: &Choice,
    root: &Path,
    opts: &Options,
    tty: bool,
    prefs: &Preferences,
) -> Result<Option<String>, String> {
    let agents = ide_agents(choice, &crate::setup::Machine::scan());
    if let Some(agent) = opts.agent.as_deref().or_else(|| {
        (!opts.launcher_setup)
            .then(|| prefs.agents.get(choice.id).map(String::as_str))
            .flatten()
    }) {
        let agent = if agent == "jazyk" { "embedded" } else { agent };
        if agents.iter().any(|(id, _)| *id == agent) {
            return Ok(Some(agent.into()));
        }
        return Err(format!(
            "agent `{agent}` is unavailable in {}; run jazyk --setup",
            choice.name
        ));
    }
    if !tty || agents.len() == 1 {
        return Ok(Some("embedded".into()));
    }
    println!("Agent in {}", choice.name);
    let groups = [crate::menu::Group {
        heading: "Agents".into(),
        rows: agents
            .iter()
            .map(|(id, name)| crate::menu::Row {
                key: id.to_string(),
                name: name.to_string(),
                note: String::new(),
            })
            .collect(),
    }];
    Ok(
        crate::menu::pick_preview(&groups, (0, 0), &|agent| ide_preview(choice, agent, root))
            .map(|(_, r)| agents[r].0.to_string()),
    )
}

fn ide_agents(
    choice: &Choice,
    machine: &crate::setup::Machine,
) -> Vec<(&'static str, &'static str)> {
    let mut out = vec![("embedded", "Jazyk")];
    let host = crate::setup::host_of(choice.id);
    for agent in crate::setup::AGENTS {
        let supported = if choice.id == "paseo" {
            matches!(agent.name, "claude" | "codex")
        } else {
            let supported_ui = match host {
                Some("vscode" | "cursor") => !agent.exts.is_empty() || agent.host == host,
                Some("zed" | "jetbrains") => {
                    agent.host == host || matches!(agent.name, "claude" | "codex" | "opencode")
                }
                _ => false,
            };
            supported_ui
                && agent.method == crate::setup::Method::Project
                && machine.detects(agent, host)
        };
        if supported && (choice.id != "paseo" || agent.bins.iter().any(|b| which(b).is_some())) {
            out.push((agent.name, agent.label));
        }
    }
    out
}

fn configure_ide(choice: &Choice, root: &Path, agent: &str) -> Result<(), String> {
    if agent == "embedded" {
        if choice.id == "paseo" {
            return crate::paseo::configure();
        }
        let host = crate::setup::host_of(choice.id).ok_or("unsupported IDE")?;
        crate::setup::install(&crate::setup::Item::Jazyk(host), root, false, true)?;
        // Pin the built-in agent in this registration; project executor settings stay independent.
        if let Some(crate::acp::install::Ide {
            registry:
                crate::acp::install::Registry::Map {
                    path,
                    key,
                    mut entry,
                },
            ..
        }) = crate::acp::install::ide(host, &crate::acp::install::spawn_command())
        {
            entry["args"] = json!(["acp", "--agent", "embedded"]);
            crate::acp::install::merge_into(&path, key, "Jazyk", &entry)?;
        }
    } else {
        setup_mcp(root, agent)?;
        if matches!(crate::setup::host_of(choice.id), Some("zed" | "jetbrains"))
            && matches!(agent, "claude" | "codex" | "opencode")
        {
            let proj = crate::project::Project::load(root);
            let profile = crate::acp::config::resolve_acp(
                Some(agent),
                &proj.acp,
                &crate::project::load_global_acp(),
                |_| None,
            )?;
            if let Some(crate::acp::install::Ide {
                registry: crate::acp::install::Registry::Map { path, key, .. },
                ..
            }) =
                crate::setup::host_of(choice.id).and_then(|h| crate::acp::install::ide(h, "jazyk"))
            {
                let mut entry = json!({"command":profile.command,"args":profile.args,"env":profile.env.into_iter().collect::<std::collections::BTreeMap<_,_>>()});
                if choice.id == "zed" {
                    entry["type"] = json!("custom");
                }
                crate::acp::install::merge_into(
                    &path,
                    key,
                    crate::setup::agent_by_name(agent).unwrap().label,
                    &entry,
                )?;
            }
        }
        if matches!(choice.id, "vscode" | "cursor") {
            let editor = if choice.id == "vscode" {
                "code"
            } else {
                "cursor"
            };
            if let Some(ext) = crate::setup::agent_by_name(agent).and_then(|a| a.exts.first()) {
                extension(editor, ext, None)?;
            }
        }
    }
    Ok(())
}

fn target_preview(id: &str) -> String {
    match id {
        "gui" => "Open Jazyk Web".into(),
        id if choice(id).is_some_and(|c| c.kind == Kind::Ide) => {
            format!("Choose an agent and open {}", choice(id).unwrap().name)
        }
        id => format!(
            "MCP in {}; run {}",
            crate::setup::agent_by_name(id)
                .map(|a| a.file)
                .unwrap_or("agent configuration"),
            choice(id).map(|c| c.name).unwrap_or(id)
        ),
    }
}

fn display_path(path: &Path) -> String {
    dirs::home_dir()
        .and_then(|home| {
            path.strip_prefix(home)
                .ok()
                .map(|p| format!("~/{}", p.display()))
        })
        .unwrap_or_else(|| path.display().to_string())
}

fn ide_preview(choice: &Choice, agent: &str, root: &Path) -> String {
    let lsp = match choice.id {
        "paseo" => {
            return if agent == "embedded" {
                format!(
                    "Register Jazyk ACP in {} and open Paseo",
                    display_path(&crate::paseo::config_path())
                )
            } else {
                format!(
                    "Configure MCP in {} and open Paseo",
                    crate::setup::agent_by_name(agent)
                        .map(|a| a.file)
                        .unwrap_or("agent configuration")
                )
            }
        }
        "vscode" | "cursor" => display_path(&crate::editor_lsp::vscode_settings(choice.id)),
        "zed" => ".zed/settings.json".into(),
        _ => ".jazyk/local/ide.json".into(),
    };
    if agent == "embedded" {
        let acp = crate::setup::host_of(choice.id)
            .and_then(|h| crate::acp::install::ide(h, "jazyk"))
            .and_then(|i| match i.registry {
                crate::acp::install::Registry::Map { path, .. } => Some(display_path(&path)),
                _ => None,
            })
            .unwrap_or_default();
        if acp == lsp {
            format!("LSP + ACP in {lsp}; install Jazyk")
        } else {
            format!("LSP in {lsp}; ACP in {acp}; install Jazyk")
        }
    } else {
        let file = crate::setup::agent_by_name(agent)
            .map(|a| {
                if a.name == "opencode"
                    && root.join("opencode.jsonc").is_file()
                    && !root.join("opencode.json").is_file()
                {
                    "opencode.jsonc"
                } else {
                    a.file
                }
            })
            .unwrap_or("agent configuration");
        format!("MCP in {file}; LSP in {lsp}; open {}", choice.name)
    }
}

// The menu: one group per kind, a row per choice, `not installed` beside a program
// that cannot be found.
fn menu_groups() -> Vec<crate::menu::Group> {
    GROUPS
        .iter()
        .map(|(kind, heading)| crate::menu::Group {
            heading: heading.to_string(),
            rows: CHOICES
                .iter()
                .filter(|c| {
                    (c.kind == *kind || (*kind == Kind::Ide && c.kind == Kind::Web))
                        && program(c.id, Path::new(".")).is_some()
                })
                .map(|c| crate::menu::Row {
                    key: c.id.to_string(),
                    name: c.name.to_string(),
                    note: String::new(),
                })
                .collect(),
        })
        .filter(|g: &crate::menu::Group| !g.rows.is_empty())
        .collect()
}

// The plain list for a terminal that cannot take keys: each choice with what it runs.
pub fn catalog() -> String {
    let mut s = String::new();
    for (kind, heading) in GROUPS {
        s.push_str(&format!("{}:\n", heading));
        for c in CHOICES
            .iter()
            .filter(|c| c.kind == kind || (kind == Kind::Ide && c.kind == Kind::Web))
        {
            s.push_str(&format!("  {} (jazyk {})\n", c.name, c.id));
        }
    }
    s
}

pub fn is_jetbrains(id: &str) -> bool {
    JETBRAINS.iter().any(|(s, _)| *s == id)
}

// An application bundle name of a JetBrains IDE.
pub fn is_jetbrains_app(name: &str) -> bool {
    JETBRAINS
        .iter()
        .any(|(_, prefixes)| prefixes.iter().any(|p| name.starts_with(p)))
}

// Whether an ACP host family has an installed, startable IDE.
pub fn host_installed(host: &str) -> bool {
    let here = Path::new(".");
    match host {
        "jetbrains" => JETBRAINS.iter().any(|(id, _)| program(id, here).is_some()),
        h => program(h, here).is_some(),
    }
}

// The ACP Client extension that hosts the Jazyk agent in VS Code and Cursor.
// Mirrors docs/frontends/acp.md#registration.
pub fn jazyk_agent_extension(host: &str) -> Result<(), String> {
    match host {
        "vscode" => extension("code", "formulahendry.acp-client", None),
        "cursor" => extension("cursor", "formulahendry.acp-client", None),
        _ => Ok(()),
    }
}

// `jazyk.toml` and `.jazyk/`, what `jazyk init` writes, without its questions. Never
// in the home directory (where `.jazyk/` is the global config) or the root.
fn init_here(cwd: &Path) -> Result<(), String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let canon = |p: &Path| crate::vfs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    if cwd.parent().is_none() || home.as_deref().map(canon) == Some(canon(cwd)) {
        return Err(format!(
            "{} is not a project folder; run this from the folder of the project",
            cwd.display()
        ));
    }
    crate::document_sets::atomic_config(cwd, crate::cli::INIT_TOML, &crate::model::hash_hex(""))
        .map_err(|(_, e)| format!("cannot write jazyk.toml: {}", e))?;
    crate::cli::init_scaffold(cwd)?;
    println!(
        "jazyk: initialized {}",
        cwd.join(crate::document_sets::CONFIG).display()
    );
    Ok(())
}

// Set one choice up: idempotent, correcting stale entries in place.
pub fn setup(choice: &Choice, root: &Path, _opts: &Options, _tty: bool) -> Result<(), String> {
    match choice.id {
        "gui" | "paseo" => Ok(()),
        "vscode" => jazyk_extension("code").and_then(|_| crate::editor_lsp::vscode("vscode")),
        "cursor" => jazyk_extension("cursor").and_then(|_| crate::editor_lsp::vscode("cursor")),
        "zed" => crate::editor_lsp::zed(root),
        id if is_jetbrains(id) => crate::editor_lsp::jetbrains(root, program(id, root)),
        id => setup_mcp(root, id),
    }
}

// The MCP entry one coding agent reads. Mirrors docs/frontends/cli.md#jazyk.
pub fn setup_mcp(root: &Path, id: &str) -> Result<(), String> {
    let a =
        crate::setup::agent_by_name(id).ok_or_else(|| format!("`{}` takes no MCP entry", id))?;
    let item = crate::setup::Item::Agent(a.name);
    if crate::setup::state(&item, root) == crate::setup::State::Installed {
        return Ok(());
    }
    crate::setup::install_launcher_agent(a.name, root)?;
    println!("jazyk: set up jazyk tools for {} ({})", a.label, a.file);
    Ok(())
}

// Claude Code connects to a `.mcp.json` server once it is approved; the approval in
// `.claude/settings.local.json` holds once the folder is trusted.
pub fn approve_claude(root: &Path) -> Result<(), String> {
    let rel = ".claude/settings.local.json";
    let path = root.join(rel);
    let mut doc: Value = match crate::vfs::read_to_string(&path) {
        Ok(text) if !text.trim().is_empty() => {
            serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON: {}", rel, e))?
        }
        _ => json!({}),
    };
    let Some(obj) = doc.as_object_mut() else {
        return Err(format!("{} does not hold a JSON object", rel));
    };
    let list = obj
        .entry("enabledMcpjsonServers")
        .or_insert_with(|| json!([]));
    let Some(items) = list.as_array_mut() else {
        return Err(format!("{}: enabledMcpjsonServers is not a list", rel));
    };
    if items.iter().any(|v| v == "jazyk") {
        return Ok(());
    }
    items.push(json!("jazyk"));
    if let Some(parent) = path.parent() {
        crate::vfs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    crate::vfs::write(&path, text + "\n").map_err(|e| format!("cannot write {}: {}", rel, e))?;
    Ok(())
}

// The approval goes with its entry; a file left empty is deleted.
// Mirrors docs/frontends/cli.md#jazyk-setup.
pub fn unapprove_claude(root: &Path) -> Result<(), String> {
    let rel = ".claude/settings.local.json";
    let path = root.join(rel);
    let Ok(text) = crate::vfs::read_to_string(&path) else {
        return Ok(());
    };
    let Ok(mut doc) = serde_json::from_str::<Value>(&text) else {
        return Ok(());
    };
    let Some(obj) = doc.as_object_mut() else {
        return Ok(());
    };
    let Some(list) = obj
        .get_mut("enabledMcpjsonServers")
        .and_then(|v| v.as_array_mut())
    else {
        return Ok(());
    };
    let before = list.len();
    list.retain(|v| v != "jazyk");
    if list.len() == before {
        return Ok(());
    }
    if list.is_empty() {
        obj.remove("enabledMcpjsonServers");
    }
    if obj.is_empty() {
        return crate::vfs::remove_file(&path).map_err(|e| format!("cannot remove {}: {}", rel, e));
    }
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    crate::vfs::write(&path, text + "\n").map_err(|e| format!("cannot write {}: {}", rel, e))
}

// Codex reads `.codex/config.toml` in a folder it trusts, and asks for that trust on
// its first start there. The merge edits one table and keeps the rest of the file.
pub fn setup_codex(path: &Path, cmd: &str) -> Result<(), String> {
    let rel = path.display().to_string();
    let path = path.to_path_buf();
    let old = crate::vfs::read_to_string(&path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = old
        .parse()
        .map_err(|e| format!("{} is not valid TOML: {}", rel, e))?;
    if doc.get("mcp_servers").is_none() {
        let mut servers = toml_edit::Table::new();
        servers.set_implicit(true);
        doc["mcp_servers"] = toml_edit::Item::Table(servers);
    }
    let servers = doc["mcp_servers"]
        .as_table_mut()
        .ok_or_else(|| format!("{}: mcp_servers is not a table", rel))?;
    let current = servers
        .get("jazyk")
        .and_then(|t| t.as_table_like())
        .is_some_and(|t| {
            t.get("command").and_then(|v| v.as_str()) == Some(cmd)
                && t.get("args").and_then(|v| v.as_array()).is_some_and(|a| {
                    a.iter().map(|v| v.as_str()).collect::<Vec<_>>() == [Some("mcp"), Some("all")]
                })
        });
    if current {
        return Ok(());
    }
    let mut entry = toml_edit::Table::new();
    entry["command"] = toml_edit::value(cmd);
    let mut args = toml_edit::Array::new();
    args.push("mcp");
    args.push("all");
    entry["args"] = toml_edit::value(args);
    servers.insert("jazyk", toml_edit::Item::Table(entry));
    crate::vfs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    crate::vfs::write(&path, doc.to_string())
        .map_err(|e| format!("cannot write {}: {}", rel, e))?;
    Ok(())
}

// Remove `[mcp_servers.jazyk]`; an emptied table goes too, and an emptied file.
// Mirrors docs/frontends/cli.md#jazyk-setup.
pub fn remove_codex(path: &Path) -> Result<bool, String> {
    let Ok(old) = crate::vfs::read_to_string(path) else {
        return Ok(false);
    };
    let mut doc: toml_edit::DocumentMut = old
        .parse()
        .map_err(|e| format!("{} is not valid TOML: {}", path.display(), e))?;
    let Some(servers) = doc.get_mut("mcp_servers").and_then(|t| t.as_table_mut()) else {
        return Ok(false);
    };
    if servers.remove("jazyk").is_none() {
        return Ok(false);
    }
    if servers.is_empty() {
        doc.remove("mcp_servers");
    }
    let text = doc.to_string();
    if text.trim().is_empty() {
        crate::vfs::remove_file(path)
            .map_err(|e| format!("cannot remove {}: {}", path.display(), e))?;
    } else {
        crate::vfs::write(path, text)
            .map_err(|e| format!("cannot write {}: {}", path.display(), e))?;
    }
    Ok(true)
}

// The installed version of an editor extension, None when it is not installed or the
// editor cannot be run.
fn installed_version(editor: &str, ext: &str) -> Option<String> {
    let out = std::process::Command::new(editor)
        .args(["--list-extensions", "--show-versions"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).lines().find_map(|l| {
        let (id, version) = l.trim().split_once('@')?;
        id.eq_ignore_ascii_case(ext).then(|| version.to_string())
    })
}

fn editor_runs(editor: &str) -> bool {
    std::process::Command::new(editor)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

// Install an extension (by id, or from a `.vsix`) unless the editor has it at this
// version or newer. An editor that cannot be found is skipped: starting it says so.
fn extension(editor: &str, ext: &str, vsix: Option<(&Path, &str)>) -> Result<(), String> {
    if !editor_runs(editor) {
        return Ok(());
    }
    let have = installed_version(editor, ext);
    let wanted = vsix.map(|(_, v)| v);
    let current = match (&have, wanted) {
        (Some(_), None) => true,
        (Some(h), Some(w)) => version_tuple(h) >= version_tuple(w),
        (None, _) => false,
    };
    if current {
        return Ok(());
    }
    let source = vsix
        .map(|(p, _)| p.display().to_string())
        .unwrap_or_else(|| ext.to_string());
    let status = std::process::Command::new(editor)
        .args(["--install-extension", &source, "--force"])
        .stdout(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("cannot run `{} --install-extension`: {}", editor, e))?;
    if !status.success() {
        return Err(format!(
            "`{} --install-extension {}` exited {}",
            editor, source, status
        ));
    }
    println!("jazyk: installed the {} extension into {}", ext, editor);
    Ok(())
}

fn version_tuple(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

// The Jazyk extension, from the newest `.vsix` in the checkout this binary was built
// in. Without one, the editor starts without the language server.
fn jazyk_extension(editor: &str) -> Result<(), String> {
    let (path, version) = match newest_vsix() {
        Some(artifact) => artifact,
        None => (
            crate::editor_release::asset("jazyk.vsix")?,
            crate::editor_release::VERSION.to_string(),
        ),
    };
    extension(editor, "jazyk.jazyk", Some((&path, &version)))
}

fn newest_vsix() -> Option<(PathBuf, String)> {
    let exe = crate::vfs::canonicalize(&std::env::current_exe().ok()?).ok()?;
    let dir = exe
        .ancestors()
        .map(|a| a.join("editors/vscode"))
        .chain(std::iter::once(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("editors/vscode"),
        ))
        .find(|d| d.vis_dir())?;
    std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let version = name
                .strip_prefix("jazyk-")?
                .strip_suffix(".vsix")?
                .to_string();
            Some((e.path(), version))
        })
        .max_by_key(|(_, v)| version_tuple(v))
}

use crate::setup::which;

// How to start a choice: the program and its arguments. None when it cannot be found.
fn program(id: &str, root: &Path) -> Option<(PathBuf, Vec<String>)> {
    let here = root.display().to_string();
    match id {
        "gui" => std::env::current_exe()
            .ok()
            .map(|e| (e, vec!["gui".to_string()])),
        "vscode" => which("code").map(|p| (p, vec![here])),
        "cursor" | "zed" | "paseo" => which(id).map(|p| (p, vec![here])),
        id if is_jetbrains(id) => jetbrains(id, &here),
        id => which(id).map(|p| (p, vec![])),
    }
}

// A JetBrains IDE: its Toolbox script on PATH or in the Toolbox scripts folder, else
// on macOS its application bundle. Mirrors docs/frontends/cli.md#jazyk.
fn jetbrains(id: &str, root: &str) -> Option<(PathBuf, Vec<String>)> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let toolbox = home
        .join("Library/Application Support/JetBrains/Toolbox/scripts")
        .join(id);
    if let Some(script) = which(id).or_else(|| toolbox.vis_file().then_some(toolbox)) {
        return Some((script, vec![root.to_string()]));
    }
    if !cfg!(target_os = "macos") {
        return None;
    }
    let prefixes = JETBRAINS.iter().find(|(s, _)| *s == id)?.1;
    for dir in [PathBuf::from("/Applications"), home.join("Applications")] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut apps: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                name.ends_with(".app") && prefixes.iter().any(|x| name.starts_with(x))
            })
            .collect();
        apps.sort();
        if let Some(app) = apps.pop() {
            return Some((
                PathBuf::from("open"),
                vec![
                    "-na".into(),
                    app.display().to_string(),
                    "--args".into(),
                    root.to_string(),
                ],
            ));
        }
    }
    None
}

// Start the choice in the project root. The program inherits the terminal and its
// exit code is ours.
fn start(choice: &Choice, root: &Path) -> i32 {
    if choice.id == "paseo" {
        let prefs = Preferences::load(root).unwrap_or_default();
        let id = prefs
            .agents
            .get("paseo")
            .map(String::as_str)
            .unwrap_or("embedded");
        let label = if id == "embedded" {
            "Jazyk"
        } else {
            crate::setup::agent_by_name(id)
                .map(|a| a.label)
                .unwrap_or(id)
        };
        println!("jazyk: in Paseo's project chat, choose {label}");
    }
    let Some((prog, args)) = program(choice.id, root) else {
        eprintln!(
            "jazyk: {} is not installed{}; its setup is written, so it starts once installed",
            choice.name,
            if choice.url.is_empty() {
                String::new()
            } else {
                format!(" (get it at {})", choice.url)
            }
        );
        return 1;
    };
    match std::process::Command::new(&prog)
        .args(&args)
        .current_dir(root)
        .status()
    {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("jazyk: cannot start {}: {}", prog.display(), e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::install::spawn_command;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("jazyk-start-{}-{}", name, crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read(path: PathBuf) -> Value {
        jsonc_parser::parse_to_serde_value(
            &std::fs::read_to_string(path).unwrap(),
            &Default::default(),
        )
        .unwrap()
    }

    // Every file-based MCP choice writes the entry its agent reads, and a second run
    // writes nothing. Mirrors docs/frontends/cli.md#jazyk.
    #[test]
    fn mcp_setups_write_each_agents_own_shape() {
        let dir = temp("mcp");
        let cmd = spawn_command();
        std::fs::create_dir_all(dir.join(".codex")).unwrap();
        std::fs::write(
            dir.join(".codex/config.toml"),
            "# mine\nmodel = \"o3\"\n\n[mcp_servers.other]\ncommand = \"x\"\n",
        )
        .unwrap();
        for id in [
            "claude", "codex", "cursor", "gemini", "pi", "droid", "opencode", "copilot", "zed",
            "junie", "roo",
        ] {
            setup_mcp(&dir, id).unwrap();
        }
        let codex = std::fs::read_to_string(dir.join(".codex/config.toml")).unwrap();
        assert!(codex.starts_with("# mine\nmodel = \"o3\""), "{}", codex);
        let parsed: toml::Value = toml::from_str(&codex).unwrap();
        assert_eq!(
            parsed["mcp_servers"]["jazyk"]["command"].as_str(),
            Some(cmd.as_str())
        );
        assert_eq!(
            parsed["mcp_servers"]["other"]["command"].as_str(),
            Some("x")
        );
        setup_mcp(&dir, "codex").unwrap();
        assert_eq!(
            codex,
            std::fs::read_to_string(dir.join(".codex/config.toml")).unwrap()
        );
        let stdio = json!({"command": cmd, "args": ["mcp", "all"]});
        assert_eq!(read(dir.join(".mcp.json"))["mcpServers"]["jazyk"], stdio);
        assert_eq!(
            read(dir.join(".cursor/mcp.json"))["mcpServers"]["jazyk"],
            stdio
        );
        assert_eq!(
            read(dir.join(".gemini/settings.json"))["mcpServers"]["jazyk"],
            stdio
        );
        assert_eq!(read(dir.join(".pi/mcp.json"))["mcpServers"]["jazyk"], stdio);
        assert_eq!(
            read(dir.join(".factory/mcp.json"))["mcpServers"]["jazyk"]["type"],
            "stdio"
        );
        assert_eq!(
            read(dir.join(".vscode/mcp.json"))["servers"]["jazyk"],
            json!({"type": "stdio", "command": cmd, "args": ["mcp", "all"]})
        );
        assert_eq!(
            read(dir.join(".zed/settings.json"))["context_servers"]["jazyk"],
            stdio
        );
        assert_eq!(
            read(dir.join(".junie/mcp/mcp.json"))["mcpServers"]["jazyk"],
            stdio
        );
        assert_eq!(
            read(dir.join(".roo/mcp.json"))["mcpServers"]["jazyk"],
            stdio
        );
        assert_eq!(
            read(dir.join("opencode.json"))["mcp"]["jazyk"],
            json!({"type": "local", "command": [cmd, "mcp", "all"], "enabled": true})
        );
        assert_eq!(
            read(dir.join(".claude/settings.local.json"))["enabledMcpjsonServers"],
            json!(["jazyk"])
        );
        let before = std::fs::read_to_string(dir.join(".claude/settings.local.json")).unwrap();
        setup_mcp(&dir, "claude").unwrap();
        assert_eq!(
            before,
            std::fs::read_to_string(dir.join(".claude/settings.local.json")).unwrap()
        );
        crate::vfs::remove_dir_all(&dir).ok();
    }

    // A stale entry (another command) is corrected in place; the other servers and
    // the approvals already there stay. Mirrors docs/frontends/cli.md#jazyk.
    #[test]
    fn a_stale_entry_is_corrected_and_neighbors_stay() {
        let dir = temp("stale");
        std::fs::write(
            dir.join(".mcp.json"),
            "{\n  \"mcpServers\": {\n    \"other\": {\"command\": \"x\"},\n    \"jazyk\": {\"command\": \"/old/jazyk\", \"args\": [\"mcp\", \"graph\"]}\n  }\n}\n",
        )
        .unwrap();
        crate::vfs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(
            dir.join(".claude/settings.local.json"),
            "{\"enabledMcpjsonServers\": [\"other\"], \"x\": 1}",
        )
        .unwrap();
        setup_mcp(&dir, "claude").unwrap();
        let mcp = read(dir.join(".mcp.json"));
        assert_eq!(mcp["mcpServers"]["jazyk"]["args"], json!(["mcp", "all"]));
        assert_eq!(mcp["mcpServers"]["other"]["command"], "x");
        let settings = read(dir.join(".claude/settings.local.json"));
        assert_eq!(settings["enabledMcpjsonServers"], json!(["other", "jazyk"]));
        assert_eq!(settings["x"], 1);
        crate::vfs::remove_dir_all(&dir).ok();
    }

    // The menu and the commands agree: every choice is in one group, no choice
    // shadows a jazyk command, and the init list names only MCP choices.
    #[test]
    fn choices_are_grouped_and_never_shadow_a_command() {
        for c in CHOICES {
            assert!(
                c.kind == Kind::Web || GROUPS.iter().any(|(k, _)| *k == c.kind),
                "{}",
                c.id
            );
            if c.id != "gui" {
                assert!(
                    crate::launcher::command_shape(c.id)
                        .is_some_and(|(n, extra)| n == Some(0) && extra.is_empty()),
                    "`{}` is not a choice command",
                    c.id
                );
            }
        }
        for id in MCP_CHOICES {
            assert!(choice(id).is_some(), "{}", id);
        }
        let list = catalog();
        for c in CHOICES {
            assert!(list.contains(c.name), "{}", c.id);
        }
    }

    #[test]
    fn launcher_preferences_are_local_and_keep_each_ide_agent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".jazyk")).unwrap();
        std::fs::write(dir.path().join(".jazyk/jazyk.toml"), "version = 1\n").unwrap();
        let mut prefs = Preferences::load(dir.path()).unwrap();
        prefs.target = "paseo".into();
        prefs.agents.insert("vscode".into(), "claude".into());
        prefs.agents.insert("paseo".into(), "embedded".into());
        prefs.save(dir.path()).unwrap();
        let restored = Preferences::load(dir.path()).unwrap();
        assert_eq!(restored.target, "paseo");
        assert_eq!(restored.agents["vscode"], "claude");
        assert_eq!(restored.agents["paseo"], "embedded");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".jazyk/jazyk.toml")).unwrap(),
            "version = 1\n"
        );
        std::fs::write(dir.path().join(".jazyk/local/launcher.toml"), "broken = [").unwrap();
        assert!(Preferences::load(dir.path()).is_err());
        assert_eq!(choice("code").unwrap().id, "vscode");
    }
    #[test]
    fn launcher_mcp_writes_only_the_selected_agents_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_mcp(dir.path(), "codex").unwrap();
        assert!(dir.path().join(".codex/config.toml").is_file());
        assert!(!dir.path().join(".mcp.json").exists());
        assert!(!dir.path().join(".claude/settings.local.json").exists());
        let before = std::fs::read(dir.path().join(".codex/config.toml")).unwrap();
        setup_mcp(dir.path(), "codex").unwrap();
        assert_eq!(
            before,
            std::fs::read(dir.path().join(".codex/config.toml")).unwrap()
        );
        assert!(target_preview("codex").contains(".codex/config.toml"));
    }

    // Home is never initialized as a project: `.jazyk/` there is the global config.
    #[test]
    fn home_is_never_initialized() {
        if let Some(home) = std::env::var_os("HOME") {
            assert!(init_here(Path::new(&home)).is_err());
        }
        assert!(init_here(Path::new("/")).is_err());
    }
}
