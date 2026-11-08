use crate::vfs::VPathExt;
// Agent setup: which coding agents are installed, which of them work inside an IDE,
// how each one gets the `jazyk` MCP entry, and where the person reaches each one
// afterwards. The launcher's IDE choices and `jazyk init` share it.
// Mirrors docs/frontends/mcp.md#agent-setup and docs/frontends/cli.md#ide-agents.
use crate::acp::install::{merge_into, spawn_command};
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Method {
    // The agent's project config, in the repository.
    Project,
    // The agent's user config; asks before writing.
    Global,
    // Application state jazyk must not write: the summary prints where to paste.
    Guided(&'static str),
}

pub struct Agent {
    pub name: &'static str,
    pub label: &'static str,
    pub method: Method,
    // The file the setup writes, or what the guided step pastes into.
    pub file: &'static str,
    // Programs on PATH that mean the agent is installed (counts in every IDE).
    pub bins: &'static [&'static str],
    // VS Code / Cursor extension ids (counts in the IDE that has it).
    pub exts: &'static [&'static str],
    // JetBrains plugin folder name fragments (counts in JetBrains IDEs).
    pub plugins: &'static [&'static str],
    // An IDE's own agent: present whenever that IDE is.
    pub host: Option<&'static str>,
    // Where the person reaches it once set up.
    pub reach: &'static str,
}

const fn agent(
    name: &'static str,
    label: &'static str,
    method: Method,
    file: &'static str,
    bins: &'static [&'static str],
    exts: &'static [&'static str],
    plugins: &'static [&'static str],
    host: Option<&'static str>,
    reach: &'static str,
) -> Agent {
    Agent { name, label, method, file, bins, exts, plugins, host, reach }
}

// Every agent jazyk sets up, in checklist order. Mirrors docs/frontends/mcp.md#agent-setup.
pub const AGENTS: &[Agent] = &[
    agent("claude", "Claude Code", Method::Project, ".mcp.json", &["claude"], &["anthropic.claude-code"], &["claude"], None,
        "jazyk tools load once Claude Code trusts the folder"),
    agent("codex", "Codex", Method::Project, ".codex/config.toml", &["codex"], &["openai.chatgpt"], &[], None,
        "jazyk tools load once Codex trusts the folder"),
    agent("gemini", "Gemini", Method::Project, ".gemini/settings.json", &["gemini"], &["google.geminicodeassist"], &[], None,
        "`/mcp` in the agent lists jazyk"),
    agent("opencode", "OpenCode", Method::Project, "opencode.json", &["opencode"], &[], &[], None,
        "jazyk tools load on the agent's next start"),
    agent("pi", "Pi", Method::Project, ".pi/mcp.json", &["pi"], &[], &[], None,
        "jazyk tools load on the agent's next start"),
    agent("droid", "Factory Droid", Method::Project, ".factory/mcp.json", &["droid"], &[], &[], None,
        "jazyk tools load on the agent's next start"),
    agent("cursor", "Cursor agent", Method::Project, ".cursor/mcp.json", &[], &[], &[], Some("cursor"),
        "Cursor Settings, MCP: jazyk, enabled"),
    // VS Code ships Copilot Chat built in; the extension id covers older installs.
    agent("copilot", "GitHub Copilot", Method::Project, ".vscode/mcp.json", &[], &["github.copilot-chat"], &[], Some("vscode"),
        "Chat in Agent mode: jazyk in the tools picker"),
    agent("zed", "Zed agent", Method::Project, ".zed/settings.json", &[], &[], &[], Some("zed"),
        "the Agent panel settings, MCP servers: jazyk"),
    agent("junie", "Junie", Method::Project, ".junie/mcp/mcp.json", &[], &[], &["junie"], None,
        "Junie's MCP servers: jazyk"),
    agent("roo", "Roo Code", Method::Project, ".roo/mcp.json", &[], &["rooveterinaryinc.roo-cline"], &[], None,
        "Roo's MCP servers: jazyk"),
    agent("copilot-cli", "GitHub Copilot CLI", Method::Global, "~/.copilot/mcp-config.json", &["copilot"], &[], &[], None,
        "`/mcp` in the agent lists jazyk"),
    agent("ai-assistant", "JetBrains AI Assistant", Method::Project, ".ai/mcp/mcp.json", &[], &[], &["ml-llm"], None,
        "Settings, Tools, AI Assistant, Model Context Protocol (MCP): jazyk"),
    agent("cline", "Cline", Method::Guided("the MCP Servers icon, Configure, Configure MCP Servers"),
        "", &[], &["saoudrizwan.claude-dev"], &[], None, "Cline's MCP servers: jazyk"),
];

// Known coding agents with no setup: listed dim as `unsupported` when installed.
pub const UNSUPPORTED: &[(&str, &str)] = &[
    ("aider", "Aider"),
    ("amp", "Amp"),
    ("goose", "Goose"),
    ("kiro-cli", "Kiro CLI"),
    ("qwen", "Qwen Code"),
    ("auggie", "Auggie"),
    ("crush", "Crush"),
];

pub fn agent_by_name(name: &str) -> Option<&'static Agent> {
    AGENTS.iter().find(|a| a.name == name)
}

// The IDE families that host the Jazyk agent, by the ACP client id `jazyk acp
// install` writes for.
pub const ACP_HOSTS: &[&str] = &["vscode", "cursor", "zed", "jetbrains"];

// The ACP client id of a launcher IDE choice.
pub fn host_of(choice: &str) -> Option<&'static str> {
    match choice {
        "vscode" => Some("vscode"),
        "cursor" => Some("cursor"),
        "zed" => Some("zed"),
        id if crate::start::is_jetbrains(id) => Some("jetbrains"),
        _ => None,
    }
}

pub fn host_label(host: &str) -> &'static str {
    match host {
        "vscode" => "VS Code",
        "cursor" => "Cursor",
        "zed" => "Zed",
        _ => "JetBrains IDEs",
    }
}

// Where the Jazyk agent is reached in each host. Mirrors docs/frontends/mcp.md#reaching-each-agent.
pub fn jazyk_reach(host: &str) -> &'static str {
    match host {
        "vscode" | "cursor" => "the ACP Client icon in the Activity Bar, Agents view: Jazyk",
        "zed" => "the Agent panel, the new thread menu: Jazyk",
        _ => "the AI Chat window, the agent selector at the bottom: Jazyk",
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

// A program on PATH, the one lookup every launcher, setup, and install check shares.
// On Windows it honors PATHEXT (`code` finds `code.cmd`). Mirrors docs/frontends/cli.md#jazyk.
pub fn which(name: &str) -> Option<PathBuf> {
    which::which(name).ok()
}

// Installed extension ids (lowercase, version stripped) of a VS Code-family IDE.
fn extensions(host: &str) -> Vec<String> {
    let dir = match host {
        "vscode" => home().join(".vscode/extensions"),
        "cursor" => home().join(".cursor/extensions"),
        _ => return Vec::new(),
    };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_lowercase())
        .collect()
}

fn has_extension(installed: &[String], id: &str) -> bool {
    let id = id.to_lowercase();
    // Folders are `<publisher>.<name>-<version>[-<platform>]`: the version starts
    // with a digit, so `github.copilot` never matches `github.copilot-chat-…`.
    installed.iter().any(|d| {
        d == &id
            || d
                .strip_prefix(&format!("{}-", id))
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
    })
}

// Plugin folder names (lowercase) across every installed JetBrains IDE: the user
// plugin folders and the plugins bundled in each application.
fn jetbrains_plugins() -> Vec<String> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let config = if cfg!(target_os = "macos") {
        home().join("Library/Application Support/JetBrains")
    } else {
        home().join(".local/share/JetBrains")
    };
    if let Ok(entries) = std::fs::read_dir(&config) {
        for e in entries.filter_map(|e| e.ok()) {
            dirs.push(e.path().join("plugins"));
            dirs.push(e.path());
        }
    }
    if cfg!(target_os = "macos") {
        for apps in [PathBuf::from("/Applications"), home().join("Applications")] {
            let Ok(entries) = std::fs::read_dir(&apps) else { continue };
            for e in entries.filter_map(|e| e.ok()) {
                let p = e.path();
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                if name.ends_with(".app") && crate::start::is_jetbrains_app(&name) {
                    dirs.push(p.join("Contents/plugins"));
                }
            }
        }
    }
    let mut out = Vec::new();
    for d in dirs {
        if let Ok(entries) = std::fs::read_dir(&d) {
            out.extend(entries.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_lowercase()));
        }
    }
    out
}

// What is installed on this machine, read once.
pub struct Machine {
    pub hosts: Vec<&'static str>,
    exts: Vec<(&'static str, Vec<String>)>,
    plugins: Vec<String>,
}

impl Machine {
    pub fn scan() -> Machine {
        let hosts: Vec<&'static str> = ACP_HOSTS
            .iter()
            .copied()
            .filter(|h| crate::start::host_installed(h))
            .collect();
        let exts = ["vscode", "cursor"].iter().map(|h| (*h, extensions(h))).collect();
        let plugins = if hosts.contains(&"jetbrains") { jetbrains_plugins() } else { Vec::new() };
        Machine { hosts, exts, plugins }
    }

    fn exts_of(&self, host: &str) -> &[String] {
        self.exts.iter().find(|(h, _)| *h == host).map(|(_, e)| e.as_slice()).unwrap_or(&[])
    }

    // Whether an agent works inside `host` (None: anywhere on this machine).
    pub fn detects(&self, a: &Agent, host: Option<&str>) -> bool {
        if a.bins.iter().any(|b| which(b).is_some()) {
            return true;
        }
        let hosts: Vec<&str> = match host {
            Some(h) => vec![h],
            None => self.hosts.clone(),
        };
        hosts.iter().any(|h| {
            a.host == Some(*h)
                || (matches!(*h, "vscode" | "cursor") && a.exts.iter().any(|x| has_extension(self.exts_of(h), x)))
                || (*h == "jetbrains" && a.plugins.iter().any(|p| self.plugins.iter().any(|d| d.contains(p))))
        })
    }
}

// One checklist row: the Jazyk agent in an IDE, a coding agent, or an installed agent
// jazyk cannot set up.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Jazyk(&'static str),
    Agent(&'static str),
    Unsupported(&'static str),
}

impl Item {
    pub fn enabled(&self) -> bool {
        !matches!(self, Item::Unsupported(_))
    }
    pub fn needs_consent(&self) -> bool {
        matches!(self, Item::Agent(n) if agent_by_name(n).is_some_and(|a| a.method == Method::Global))
    }
    pub fn row(&self, one_host: bool) -> crate::menu::Row {
        let (key, name, note) = match self {
            Item::Jazyk(h) => (
                format!("jazyk-{}", h),
                if one_host { "Jazyk agent (ACP)".to_string() } else { format!("Jazyk agent in {}", host_label(h)) },
                "global registration".to_string(),
            ),
            Item::Agent(n) => {
                let a = agent_by_name(n).unwrap();
                let note = match a.method {
                    Method::Project => a.file.to_string(),
                    Method::Global => format!("{} (asks first)", a.file),
                    Method::Guided(_) => "paste one entry".to_string(),
                };
                (n.to_string(), a.label.to_string(), note)
            }
            Item::Unsupported(b) => {
                let label = UNSUPPORTED.iter().find(|u| u.0 == *b).map(|u| u.1).unwrap_or(b);
                (b.to_string(), label.to_string(), "unsupported".to_string())
            }
        };
        crate::menu::Row { key, name, note }
    }
}

// The checklist for one IDE (Some) or for every installed IDE (None, `jazyk init`).
pub fn items(m: &Machine, host: Option<&'static str>) -> Vec<Item> {
    let mut out = Vec::new();
    match host {
        Some(h) => out.push(Item::Jazyk(h)),
        None => out.extend(m.hosts.iter().map(|h| Item::Jazyk(h))),
    }
    for a in AGENTS {
        if m.detects(a, host) {
            out.push(Item::Agent(a.name));
        }
    }
    for (bin, _) in UNSUPPORTED {
        if which(bin).is_some() {
            out.push(Item::Unsupported(bin));
        }
    }
    out
}

// The state of one row's setup. Mirrors docs/frontends/cli.md#ide-agents.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Installed,
    Stale,
    Missing,
    // A guided step: jazyk cannot read it back.
    Unknown,
}

impl State {
    pub fn present(self) -> bool {
        matches!(self, State::Installed | State::Stale)
    }
    pub fn word(self) -> &'static str {
        match self {
            State::Installed => "installed",
            State::Stale => "stale",
            State::Missing => "missing",
            State::Unknown => "unknown",
        }
    }
}

// Where one entry lives.
enum Spec {
    Json { path: PathBuf, key: &'static str, name: &'static str, entry: serde_json::Value },
    Codex { path: PathBuf, cmd: String },
}

fn stdio_entry(cmd: &str) -> serde_json::Value {
    json!({"command": cmd, "args": ["mcp", "all"]})
}

fn global_file(name: &str) -> Option<PathBuf> {
    match name {
        "copilot-cli" => Some(
            std::env::var_os("COPILOT_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".copilot"))
                .join("mcp-config.json"),
        ),
        _ => None,
    }
}

// The entries a row's setup consists of. Mirrors docs/frontends/mcp.md#agent-setup.
fn specs(item: &Item, root: &Path, cmd: &str) -> Vec<Spec> {
    let json_at = |rel: &str, key: &'static str, entry: serde_json::Value| Spec::Json {
        path: root.join(rel),
        key,
        name: "jazyk",
        entry,
    };
    let stdio = stdio_entry(cmd);
    match item {
        Item::Jazyk(h) => match crate::acp::install::ide(h, cmd).map(|i| i.registry) {
            Some(crate::acp::install::Registry::Map { path, key, entry }) => {
                vec![Spec::Json { path, key, name: "Jazyk", entry }]
            }
            _ => Vec::new(),
        },
        Item::Unsupported(_) => Vec::new(),
        // `.mcp.json` is the canonical project file: every project row writes it too.
        // Mirrors docs/frontends/mcp.md#agent-setup.
        Item::Agent(n) if *n != "claude" && agent_by_name(n).is_some_and(|a| a.method == Method::Project) => {
            let mut out = vec![json_at(CANONICAL, "mcpServers", stdio.clone())];
            out.extend(own_specs(n, root, cmd));
            out
        }
        Item::Agent(n) => own_specs(n, root, cmd),
    }
}

const CANONICAL: &str = ".mcp.json";

// A row's own file, without the canonical `.mcp.json` its project setup adds.
fn own_specs(n: &str, root: &Path, cmd: &str) -> Vec<Spec> {
    let json_at = |rel: &str, key: &'static str, entry: serde_json::Value| Spec::Json {
        path: root.join(rel),
        key,
        name: "jazyk",
        entry,
    };
    let stdio = stdio_entry(cmd);
    let typed = json!({"type": "stdio", "command": cmd, "args": ["mcp", "all"]});
    match n {
            "claude" => vec![json_at(".mcp.json", "mcpServers", stdio)],
            "cursor" => vec![json_at(".cursor/mcp.json", "mcpServers", stdio)],
            "gemini" => vec![json_at(".gemini/settings.json", "mcpServers", stdio)],
            "pi" => vec![json_at(".pi/mcp.json", "mcpServers", stdio)],
            "junie" => vec![json_at(".junie/mcp/mcp.json", "mcpServers", stdio)],
            "roo" => vec![json_at(".roo/mcp.json", "mcpServers", stdio)],
            "ai-assistant" => vec![json_at(".ai/mcp/mcp.json", "mcpServers", stdio)],
            "zed" => vec![json_at(".zed/settings.json", "context_servers", stdio)],
            "copilot" => vec![json_at(".vscode/mcp.json", "servers", typed)],
            "droid" => vec![json_at(".factory/mcp.json", "mcpServers", typed)],
            "opencode" => {
                let rel = if root.join("opencode.jsonc").vexists() && !root.join("opencode.json").vexists() {
                    "opencode.jsonc"
                } else {
                    "opencode.json"
                };
                vec![json_at(rel, "mcp", json!({"type": "local", "command": [cmd, "mcp", "all"], "enabled": true}))]
            }
            "codex" => vec![Spec::Codex { path: root.join(".codex/config.toml"), cmd: cmd.to_string() }],
            n => match global_file(n) {
                Some(path) => vec![Spec::Json { path, key: "mcpServers", name: "jazyk", entry: stdio }],
                None => Vec::new(),
            },
    }
}

// Whether another agent row still relies on the canonical `.mcp.json` entry.
fn canonical_in_use(except: &str, root: &Path) -> bool {
    let cmd = spawn_command();
    AGENTS.iter().filter(|a| a.name != except && a.method == Method::Project).any(|a| {
        if a.name == "claude" {
            return claude_approved(root);
        }
        own_specs(a.name, root, &cmd).iter().any(|s| spec_state(s) != State::Missing)
    })
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let text = crate::vfs::read_to_string(path).ok()?;
    jsonc_parser::parse_to_serde_value(&text, &Default::default()).ok().flatten()
}

fn codex_entry(path: &Path) -> Option<(Option<String>, Vec<String>)> {
    let text = crate::vfs::read_to_string(path).ok()?;
    let doc: toml::Value = toml::from_str(&text).ok()?;
    let t = doc.get("mcp_servers")?.get("jazyk")?;
    let cmd = t.get("command").and_then(|v| v.as_str()).map(String::from);
    let args = t
        .get("args")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    Some((cmd, args))
}

fn spec_state(spec: &Spec) -> State {
    match spec {
        Spec::Json { path, key, name, entry } => match read_json(path).as_ref().and_then(|v| v.get(*key)?.get(*name).cloned()) {
            None => State::Missing,
            Some(v) if &v == entry => State::Installed,
            Some(_) => State::Stale,
        },
        Spec::Codex { path, cmd } => match codex_entry(path) {
            None => State::Missing,
            Some((Some(c), args)) if &c == cmd && args == ["mcp", "all"] => State::Installed,
            Some(_) => State::Stale,
        },
    }
}

fn claude_approved(root: &Path) -> bool {
    read_json(&root.join(".claude/settings.local.json"))
        .and_then(|v| v.get("enabledMcpjsonServers").cloned())
        .and_then(|v| v.as_array().cloned())
        .is_some_and(|a| a.iter().any(|x| x == "jazyk"))
}

// The state of one row. Mirrors docs/frontends/cli.md#ide-agents.
pub fn state(item: &Item, root: &Path) -> State {
    if let Item::Agent(n) = item {
        if matches!(agent_by_name(n).map(|a| a.method), Some(Method::Guided(_))) {
            return State::Unknown;
        }
    }
    let cmd = spawn_command();
    // A row's own file decides; the shared `.mcp.json` only completes it.
    let own = match item {
        Item::Agent(n) => own_specs(n, root, &cmd),
        _ => specs(item, root, &cmd),
    };
    let states: Vec<State> = own.iter().map(spec_state).collect();
    if states.is_empty() {
        return State::Missing;
    }
    // Claude Code's own part is its approval: the shared entry alone is another row's.
    if *item == Item::Agent("claude") && !claude_approved(root) && canonical_in_use("claude", root) {
        return State::Missing;
    }
    let mut s = if states.iter().all(|s| *s == State::Installed) {
        State::Installed
    } else if states.iter().all(|s| *s == State::Missing) {
        State::Missing
    } else {
        State::Stale
    };
    // Parts beyond the entry: the shared `.mcp.json`, the Claude approval, the ACP
    // Client extension.
    if s == State::Installed {
        let incomplete = match item {
            Item::Agent("claude") => !claude_approved(root),
            Item::Agent(_) => specs(item, root, &cmd).iter().any(|spec| spec_state(spec) != State::Installed),
            Item::Jazyk(h @ ("vscode" | "cursor")) => !has_extension(&extensions(h), "formulahendry.acp-client"),
            _ => false,
        };
        if incomplete {
            s = State::Stale;
        }
    }
    s
}

fn ask_yes(question: &str) -> bool {
    use std::io::Write;
    print!("{} [y/N] ", question);
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).ok();
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

// Set one row up, quietly. `consented` covers a global write (the person named the
// row, or it was set up before); otherwise a terminal asks.
// Mirrors docs/frontends/mcp.md#agent-setup.
pub fn install(item: &Item, root: &Path, tty: bool, consented: bool) -> Result<(), String> {
    let cmd = spawn_command();
    if let Item::Agent(n) = item {
        let a = agent_by_name(n).ok_or_else(|| format!("unknown agent `{}`", n))?;
        if a.method == Method::Global && !consented {
            let path = global_file(n).map(|p| p.display().to_string()).unwrap_or_default();
            if !(tty && ask_yes(&format!("{}: write the jazyk entry into {}?", a.label, path))) {
                return Err(format!("skipped: writing {} needs a yes", path));
            }
        }
    }
    for spec in specs(item, root, &cmd) {
        match spec {
            Spec::Json { path, key, name, entry } => {
                merge_into(&path, key, name, &entry).map_err(|e| format!("cannot write {}: {}", path.display(), e))?;
            }
            Spec::Codex { path, cmd } => crate::start::setup_codex(&path, &cmd)?,
        }
    }
    match item {
        Item::Agent("claude") => crate::start::approve_claude(root)?,
        Item::Jazyk(h) => crate::start::jazyk_agent_extension(h)?,
        _ => {}
    }
    Ok(())
}

// Remove one row's setup. Ok(false) when nothing was there.
// Mirrors docs/frontends/cli.md#jazyk-setup.
pub fn remove(item: &Item, root: &Path) -> Result<bool, String> {
    let cmd = spawn_command();
    let mut removed = false;
    // The canonical `.mcp.json` entry stays while another agent row relies on it.
    let keep_canonical = matches!(item, Item::Agent(n) if canonical_in_use(n, root));
    for spec in specs(item, root, &cmd) {
        if keep_canonical && matches!(&spec, Spec::Json { path, .. } if path == &root.join(CANONICAL)) {
            continue;
        }
        let path = match &spec {
            Spec::Json { path, .. } | Spec::Codex { path, .. } => path.clone(),
        };
        removed |= match spec {
            Spec::Json { path, key, name, .. } => crate::acp::install::remove_from(&path, key, name)
                .map_err(|e| format!("cannot edit {}: {}", path.display(), e))?,
            Spec::Codex { path, .. } => crate::start::remove_codex(&path)?,
        };
        prune_empty_dirs(&path, root);
    }
    if let Item::Agent("claude") = item {
        removed |= claude_approved(root);
        crate::start::unapprove_claude(root)?;
    }
    Ok(removed)
}

// Folders inside the project that a removal left empty (`.ai/mcp/`, `.codex/`) go
// too; nothing outside the project root is touched.
fn prune_empty_dirs(file: &Path, root: &Path) {
    let mut dir = file.parent();
    while let Some(d) = dir {
        if d == root || !d.starts_with(root) {
            break;
        }
        let empty = std::fs::read_dir(d).map(|mut e| e.next().is_none()).unwrap_or(false);
        if !empty || std::fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

// The entry a guided step pastes: the `mcpServers` shape Cline accepts.
pub fn paste_text(cmd: &str) -> String {
    serde_json::to_string(&json!({"mcpServers": {"jazyk": stdio_entry(cmd)}})).unwrap_or_default()
}

// The registration rule's fallback: an absolute path breaks when the binary moves.
pub fn warn_if_absolute() {
    let cmd = spawn_command();
    if cmd.contains(std::path::MAIN_SEPARATOR) {
        eprintln!(
            "jazyk: warning: `jazyk` on PATH is not this binary, so entries name {}; they break if it moves (put jazyk on PATH and rerun to fix)",
            cmd
        );
    }
}

pub fn label(item: &Item) -> String {
    match item {
        Item::Jazyk(_) => "Jazyk agent".to_string(),
        Item::Agent(n) => agent_by_name(n).map(|a| a.label).unwrap_or(n).to_string(),
        Item::Unsupported(b) => b.to_string(),
    }
}

// The short summary after a start or a setup: the Jazyk agent's place, the agents
// with jazyk tools, guided pastes, failures, and the command that changes it.
// Mirrors docs/frontends/cli.md#ide-agents.
pub fn print_summary(set_up: &[Item], failed: &[(Item, String)], change: &str) {
    for item in set_up {
        if let Item::Jazyk(h) = item {
            println!("  Jazyk agent: {}", jazyk_reach(h));
        }
    }
    let tools: Vec<String> = set_up
        .iter()
        .filter(|i| matches!(i, Item::Agent(n) if !matches!(agent_by_name(n).map(|a| a.method), Some(Method::Guided(_)))))
        .map(label)
        .collect();
    if !tools.is_empty() {
        println!("  jazyk tools: {}", tools.join(", "));
    }
    let cmd = spawn_command();
    for item in set_up {
        if let Item::Agent(n) = item {
            if let Some(Method::Guided(where_)) = agent_by_name(n).map(|a| a.method) {
                println!("  {}: paste {} in {}", label(item), paste_text(&cmd), where_);
            }
        }
    }
    for (item, why) in failed {
        println!("  {}: not set up ({})", label(item), why);
    }
    println!("  change with `{}`", change);
}

// The checklist on a terminal: Some(the chosen rows), None when the person quits.
// One enabled row is taken without asking. Off a terminal every row that needs no
// consent is taken. Mirrors docs/frontends/cli.md#ide-agents.
pub fn choose(items: &[Item], heading: &str, tty: bool, one_host: bool) -> Option<Vec<Item>> {
    let enabled: Vec<bool> = items.iter().map(|i| i.enabled()).collect();
    let usable: Vec<&Item> = items.iter().filter(|i| i.enabled()).collect();
    if usable.len() <= 1 {
        return Some(usable.into_iter().cloned().collect());
    }
    if !tty {
        return Some(usable.into_iter().filter(|i| !i.needs_consent()).cloned().collect());
    }
    let rows: Vec<crate::menu::Row> = items.iter().map(|i| i.row(one_host)).collect();
    let ticked = crate::menu::check(heading, &rows, &enabled, &enabled)?;
    Some(items.iter().zip(ticked).filter(|(i, t)| *t && i.enabled()).map(|(i, _)| i.clone()).collect())
}

// A name on the `jazyk setup --remove` list. Mirrors docs/frontends/cli.md#jazyk-setup.
fn named(name: &str, host: Option<&'static str>) -> Option<Item> {
    if name == "jazyk" {
        return host.map(Item::Jazyk);
    }
    if let Some(h) = name.strip_prefix("jazyk-") {
        return ACP_HOSTS.iter().find(|x| **x == h).map(|h| Item::Jazyk(h));
    }
    agent_by_name(name).map(|a| Item::Agent(a.name))
}

// `jazyk setup [ide] [--list] [--remove NAMES]`. Mirrors docs/frontends/cli.md#jazyk-setup.
pub fn run(positional: &[String], opts: &crate::cli::Options) -> i32 {
    use std::io::IsTerminal;
    let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match positional.first().map(String::as_str) {
        Some("agents-md") => return run_agents_md(&cwd),
        Some("trust") => return run_trust(&cwd),
        _ => {}
    }
    let (host, ide_name) = match positional.first() {
        None => (None, None),
        Some(id) => match host_of(id) {
            Some(h) => (Some(h), crate::start::choice(id).map(|c| c.name)),
            None => {
                eprintln!("jazyk: `{}` is not an IDE; one of vscode, cursor, zed, idea, pycharm, ...", id);
                return 2;
            }
        },
    };
    let Some(root) = crate::project::find_root(&cwd) else {
        eprintln!("jazyk: no jazyk project here; `jazyk init` makes one");
        return 1;
    };
    let change = match positional.first() {
        Some(id) => format!("jazyk setup {}", id),
        None => "jazyk setup".to_string(),
    };
    let machine = Machine::scan();
    let items = items(&machine, host);
    let one_host = host.is_some();

    if let Some(list) = opts.remove.as_deref() {
        let mut code = 0;
        let mut rest = Vec::new();
        for n in list.split(',').map(str::trim) {
            match n {
                "agents-md" => match crate::agents_md::remove(&root) {
                    Ok(true) => println!("jazyk: removed the jazyk section of AGENTS.md"),
                    Ok(false) => println!("jazyk: no jazyk section in AGENTS.md"),
                    Err(e) => {
                        eprintln!("jazyk: {e}");
                        code = 1;
                    }
                },
                "trust" => match crate::agent_context::home() {
                    Some(home) => match crate::agent_context::save_trust(&home, &root, None) {
                        Ok(()) => println!("jazyk: revoked trust for this project's MCP servers and skills"),
                        Err(e) => {
                            eprintln!("jazyk: {e}");
                            code = 1;
                        }
                    },
                    None => {
                        eprintln!("jazyk: no home directory to keep trust in");
                        code = 1;
                    }
                },
                other => rest.push(other),
            }
        }
        if rest.is_empty() {
            return code;
        }
        let list = rest.join(",");
        let targets: Vec<Item> = if list == "all" {
            items.iter().filter(|i| state(i, &root).present()).cloned().collect()
        } else {
            let mut out = Vec::new();
            for n in list.split(',').map(str::trim) {
                match named(n, host) {
                    Some(i) => out.push(i),
                    None => {
                        eprintln!("jazyk: unknown setup name `{}`", n);
                        return 2;
                    }
                }
            }
            out
        };
        return remove_all(&targets, &root).max(code);
    }

    let states: Vec<State> = items.iter().map(|i| state(i, &root)).collect();
    if opts.list || !tty {
        let width = items.iter().map(|i| i.row(one_host).name.chars().count()).max().unwrap_or(0);
        for (i, s) in items.iter().zip(&states) {
            let row = i.row(one_host);
            let word = if i.enabled() { s.word() } else { "unsupported" };
            println!("  {:w$}  {:11} {}", row.name, word, if i.enabled() { row.note } else { String::new() }, w = width);
        }
        println!("  {:w$}  {:11} AGENTS.md jazyk section", "agents-md", crate::agents_md::state(&root), w = width);
        let trust = crate::agent_context::home().map(|h| crate::agent_context::trust_state(&h, &root)).unwrap_or("none");
        println!("  {:w$}  {:11} project MCP servers and skills", "trust", trust, w = width);
        if !tty && !opts.list {
            println!("\n`{} --remove NAMES` removes; on a terminal, `{}` adds and removes.", change, change);
        }
        return 0;
    }

    let enabled: Vec<bool> = items.iter().map(|i| i.enabled()).collect();
    let any = states.iter().any(|s| s.present());
    let initial: Vec<bool> = items
        .iter()
        .zip(&states)
        .map(|(i, s)| i.enabled() && (!any || s.present()))
        .collect();
    let rows: Vec<crate::menu::Row> = items
        .iter()
        .zip(&states)
        .map(|(i, s)| {
            let mut row = i.row(one_host);
            if i.enabled() {
                row.note = format!("{:9} {}", s.word(), row.note);
            }
            row
        })
        .collect();
    let heading = match ide_name {
        Some(n) => format!("jazyk setup for {} (ticked: set up)", n),
        None => "jazyk setup (ticked: set up)".to_string(),
    };
    let Some(ticked) = crate::menu::check(&heading, &rows, &enabled, &initial) else {
        return 0;
    };
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut failed: Vec<(Item, String)> = Vec::new();
    let mut warned = false;
    for ((item, s), t) in items.iter().zip(&states).zip(&ticked) {
        if !item.enabled() {
            continue;
        }
        if *t && *s != State::Installed {
            if !warned {
                warn_if_absolute();
                warned = true;
            }
            match install(item, &root, tty, s.present()) {
                Ok(()) => added.push(label(item)),
                Err(e) => failed.push((item.clone(), e)),
            }
        } else if !*t && s.present() {
            match remove(item, &root) {
                Ok(_) => removed.push(label(item)),
                Err(e) => failed.push((item.clone(), e)),
            }
        }
    }
    if added.is_empty() && removed.is_empty() && failed.is_empty() {
        println!("jazyk: no changes");
    }
    if !added.is_empty() {
        println!("jazyk: added {}", added.join(", "));
    }
    if !removed.is_empty() {
        println!("jazyk: removed {}", removed.join(", "));
    }
    for (item, why) in &failed {
        println!("  {}: {}", label(item), why);
    }
    for item in items.iter().zip(&ticked).filter(|(_, t)| **t).map(|(i, _)| i) {
        if let Item::Agent(n) = item {
            if let Some(Method::Guided(where_)) = agent_by_name(n).map(|a| a.method) {
                println!("  {}: paste {} in {}", label(item), paste_text(&spawn_command()), where_);
            }
        }
    }
    if failed.is_empty() { 0 } else { 1 }
}

// `jazyk setup agents-md`: create or refresh the section and the imports.
// Mirrors docs/frontends/cli.md#jazyk-setup.
fn run_agents_md(cwd: &Path) -> i32 {
    let Some(root) = crate::project::find_root(cwd) else {
        eprintln!("jazyk: no jazyk project here; `jazyk init` makes one");
        return 1;
    };
    if !crate::agents_md::enabled(&root) {
        println!("jazyk: [agents] block = false in jazyk.toml; nothing written");
        return 0;
    }
    let outcome = crate::agents_md::sync(&root);
    for line in &outcome.changed {
        println!("jazyk: {line}");
    }
    for w in &outcome.warnings {
        eprintln!("jazyk: {w}");
    }
    if outcome.changed.is_empty() && outcome.warnings.is_empty() {
        println!("jazyk: the jazyk section of AGENTS.md is current");
    }
    if outcome.warnings.is_empty() { 0 } else { 1 }
}

fn describe(t: &crate::agent_context::Transport) -> String {
    match t {
        crate::agent_context::Transport::Stdio { command, args, .. } => {
            std::iter::once(command.as_str()).chain(args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
        }
        crate::agent_context::Transport::Http { url, .. } => url.clone(),
    }
}

// `jazyk setup trust`: show what the project would run and record it as trusted.
// Mirrors docs/frontends/agent-context.md#trust.
fn run_trust(cwd: &Path) -> i32 {
    let Some(root) = crate::project::find_root(cwd) else {
        eprintln!("jazyk: no jazyk project here; `jazyk init` makes one");
        return 1;
    };
    let Some(home) = crate::agent_context::home() else {
        eprintln!("jazyk: no home directory to keep trust in");
        return 1;
    };
    let env = |n: &str| crate::sys::env_var(n).ok();
    let (servers, warnings) = crate::agent_context::servers(&root, None, &env);
    let (skills, skill_warnings) = crate::agent_context::skills(&root, None);
    for w in warnings.iter().chain(&skill_warnings) {
        eprintln!("jazyk: {w}");
    }
    let trust = crate::agent_context::current_trust(&servers, &skills);
    if trust == crate::agent_context::Trust::default() {
        println!("jazyk: nothing to trust: the project names no MCP servers or skills");
        return 0;
    }
    for s in servers.iter().filter(|s| s.project) {
        println!("  server {}  {}  ({})", s.name, describe(&s.transport), s.source);
    }
    for s in skills.iter().filter(|s| s.project) {
        println!("  skill  {}  {}", s.name, s.source);
    }
    match crate::agent_context::save_trust(&home, &root, Some(&trust)) {
        Ok(()) => {
            println!("jazyk: trusted for the Jazyk agent's conversations; an edited entry asks again");
            0
        }
        Err(e) => {
            eprintln!("jazyk: {e}");
            1
        }
    }
}

fn remove_all(targets: &[Item], root: &Path) -> i32 {
    let mut code = 0;
    let mut removed = Vec::new();
    for item in targets {
        if let Item::Agent(n) = item {
            if let Some(Method::Guided(where_)) = agent_by_name(n).map(|a| a.method) {
                println!("  {}: remove jazyk by hand in {}", label(item), where_);
                continue;
            }
        }
        match remove(item, root) {
            Ok(true) => removed.push(label(item)),
            Ok(false) => {}
            Err(e) => {
                eprintln!("jazyk: {}: {}", label(item), e);
                code = 1;
            }
        }
    }
    if removed.is_empty() {
        println!("jazyk: nothing to remove");
    } else {
        println!("jazyk: removed {}", removed.join(", "));
    }
    code
}

#[cfg(test)]
mod tests {
    // One shared program lookup, from the `which` crate: what is on PATH is found,
    // and on Windows a program found through PATHEXT (`cmd` is `cmd.exe`).
    #[test]
    fn program_lookup_finds_what_is_on_path() {
        assert!(super::which("jazyk-no-such-program-anywhere").is_none());
        #[cfg(unix)]
        assert!(super::which("sh").is_some_and(|p| p.is_absolute()));
        #[cfg(windows)]
        assert!(super::which("cmd").is_some_and(|p| p.extension().is_some()));
    }

    use super::*;

    #[test]
    fn every_agent_is_named_once_and_set_up_somewhere() {
        let mut seen = std::collections::BTreeSet::new();
        for a in AGENTS {
            assert!(seen.insert(a.name), "{} twice", a.name);
            assert!(
                !a.bins.is_empty() || !a.exts.is_empty() || !a.plugins.is_empty() || a.host.is_some(),
                "{} is never detected",
                a.name
            );
            if a.method == Method::Project {
                assert!(!a.file.is_empty(), "{}", a.name);
            }
        }
    }

    #[test]
    fn extension_ids_match_versioned_folders() {
        let installed = vec!["github.copilot-chat-0.33.1".to_string(), "anthropic.claude-code-2.1.318-darwin-arm64".to_string()];
        assert!(has_extension(&installed, "GitHub.copilot-chat"));
        assert!(has_extension(&installed, "anthropic.claude-code"));
        assert!(!has_extension(&installed, "github.copilot"));
    }

    #[test]
    fn a_single_row_is_taken_without_asking_and_unsupported_never() {
        let items = vec![Item::Jazyk("zed"), Item::Unsupported("aider")];
        assert_eq!(choose(&items, "", true, true), Some(vec![Item::Jazyk("zed")]));
        let items = vec![Item::Jazyk("zed"), Item::Agent("copilot-cli"), Item::Agent("claude")];
        assert_eq!(
            choose(&items, "", false, true),
            Some(vec![Item::Jazyk("zed"), Item::Agent("claude")]),
            "off a terminal, a global write is not taken"
        );
    }

    #[test]
    fn the_paste_is_one_mcp_servers_entry() {
        let v: serde_json::Value = serde_json::from_str(&paste_text("jazyk")).unwrap();
        assert_eq!(v["mcpServers"]["jazyk"]["args"], json!(["mcp", "all"]));
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jazyk-setup-{}-{}", name, crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(&dir).unwrap();
        dir
    }

    // Install reads back as installed, an edited command as stale, and remove takes
    // only jazyk's entry, deleting a file it leaves empty.
    // Mirrors docs/frontends/cli.md#jazyk-setup.
    #[test]
    fn install_state_remove_round_trip() {
        let dir = temp("round");
        let claude = Item::Agent("claude");
        let codex = Item::Agent("codex");
        let copilot = Item::Agent("copilot");
        assert_eq!(state(&claude, &dir), State::Missing);
        std::fs::write(dir.join(".mcp.json"), "{\n  // mine\n  \"mcpServers\": {\n    \"other\": {\"command\": \"x\"}\n  }\n}\n").unwrap();
        for item in [&claude, &codex, &copilot] {
            install(item, &dir, false, false).unwrap();
            assert_eq!(state(item, &dir), State::Installed, "{:?}", item);
        }
        let mcp = std::fs::read_to_string(dir.join(".mcp.json")).unwrap();
        std::fs::write(dir.join(".mcp.json"), mcp.replace("\"all\"", "\"graph\"")).unwrap();
        assert_eq!(state(&claude, &dir), State::Stale);

        for item in [&claude, &codex, &copilot] {
            assert!(remove(item, &dir).unwrap(), "{:?}", item);
            assert_eq!(state(item, &dir), State::Missing, "{:?}", item);
            assert!(!remove(item, &dir).unwrap(), "a second remove finds nothing");
        }
        let left = std::fs::read_to_string(dir.join(".mcp.json")).unwrap();
        assert!(left.contains("// mine") && left.contains("\"other\"") && !left.contains("jazyk"), "{}", left);
        // `.mcp.json` is canonical: a codex setup writes it, and it stays while
        // another row relies on it.
        install(&codex, &dir, false, false).unwrap();
        install(&copilot, &dir, false, false).unwrap();
        assert!(std::fs::read_to_string(dir.join(".mcp.json")).unwrap().contains("jazyk"));
        assert_eq!(state(&claude, &dir), State::Missing, "the shared entry is not Claude's setup");
        remove(&codex, &dir).unwrap();
        assert!(std::fs::read_to_string(dir.join(".mcp.json")).unwrap().contains("jazyk"));
        assert_eq!(state(&codex, &dir), State::Missing);
        remove(&copilot, &dir).unwrap();
        assert!(!std::fs::read_to_string(dir.join(".mcp.json")).unwrap().contains("jazyk"));
        assert!(!dir.join(".vscode/mcp.json").exists(), "an emptied file is deleted");
        assert!(!dir.join(".codex/config.toml").exists(), "an emptied codex file is deleted");
        assert!(!dir.join(".claude/settings.local.json").exists(), "the approval goes with the entry");
        assert!(!dir.join(".vscode").exists() && !dir.join(".codex").exists(), "emptied folders go too");
        crate::vfs::remove_dir_all(&dir).ok();
    }
}
