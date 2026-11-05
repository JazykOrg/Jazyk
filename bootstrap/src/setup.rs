use crate::vfs::VPathExt;
// Agent setup: which coding agents are installed, which of them work inside an IDE,
// how each one gets the `jazyk` MCP entry, and where the person reaches each one
// afterwards. The launcher's IDE choices and `jazyk init` share it.
// Mirrors docs/frontends/mcp.md#agent-setup and docs/frontends/cli.md#ide-agents.
use crate::acp::install::{merge_into, spawn_command, Merge};
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
    agent("ai-assistant", "JetBrains AI Assistant", Method::Guided("Settings, Tools, AI Assistant, Model Context Protocol (MCP), add"),
        "", &[], &[], &["ml-llm"], None, "AI Assistant's MCP servers: jazyk"),
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

pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.vis_file())
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
                "the IDE's agent picker".to_string(),
            ),
            Item::Agent(n) => {
                let a = agent_by_name(n).unwrap();
                let note = match a.method {
                    Method::Project => a.file.to_string(),
                    Method::Global => format!("{} (asks first)", a.file),
                    Method::Guided(_) => "guided: paste one entry".to_string(),
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

// What one row's setup came to, for the summary.
pub struct Outcome {
    pub label: String,
    pub line: String,
    pub ok: bool,
    // The text to paste for a guided step.
    pub paste: Option<String>,
}

fn stdio_entry(cmd: &str) -> serde_json::Value {
    json!({"command": cmd, "args": ["mcp", "all"]})
}

// The entry a guided step pastes: the `mcpServers` shape both AI Assistant and Cline
// accept.
pub fn paste_text(cmd: &str) -> String {
    serde_json::to_string_pretty(&json!({"mcpServers": {"jazyk": stdio_entry(cmd)}})).unwrap_or_default()
}

fn ask_yes(question: &str) -> bool {
    use std::io::Write;
    print!("{} [y/N] ", question);
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).ok();
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
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

// Set one row up. `consented` is true when the person named it (a flag) or this is
// a terminal that may ask. Mirrors docs/frontends/mcp.md#agent-setup.
pub fn apply(item: &Item, root: &Path, tty: bool, consented: bool) -> Outcome {
    let cmd = spawn_command();
    match item {
        Item::Jazyk(h) => {
            let label = if *h == "jetbrains" { "Jazyk agent".to_string() } else { format!("Jazyk agent ({})", host_label(h)) };
            match crate::start::setup_jazyk_agent(h) {
                Ok(()) => Outcome { label, line: jazyk_reach(h).to_string(), ok: true, paste: None },
                Err(e) => Outcome { label, line: format!("not set up: {}", e), ok: false, paste: None },
            }
        }
        Item::Unsupported(b) => Outcome { label: b.to_string(), line: "unsupported".into(), ok: false, paste: None },
        Item::Agent(n) => {
            let a = agent_by_name(n).unwrap();
            let label = a.label.to_string();
            match a.method {
                Method::Project => match crate::start::setup_mcp(root, n) {
                    Ok(()) => Outcome { label, line: format!("ready ({}); {}", a.file, a.reach), ok: true, paste: None },
                    Err(e) => Outcome { label, line: format!("not set up: {}", e), ok: false, paste: None },
                },
                Method::Global => {
                    let Some(path) = global_file(n) else {
                        return Outcome { label, line: "no global file known".into(), ok: false, paste: None };
                    };
                    let allowed = consented
                        || (tty && ask_yes(&format!("{}: write the jazyk entry into {}?", a.label, path.display())));
                    if !allowed {
                        return Outcome {
                            label,
                            line: format!("skipped: writing {} needs a yes", path.display()),
                            ok: false,
                            paste: None,
                        };
                    }
                    match merge_into(&path, "mcpServers", "jazyk", &stdio_entry(&cmd)) {
                        Ok(m) => {
                            if matches!(m, Merge::Written) {
                                println!("jazyk: wrote the `jazyk` MCP server into {}", path.display());
                            }
                            Outcome { label, line: format!("ready ({}); {}", path.display(), a.reach), ok: true, paste: None }
                        }
                        Err(e) => Outcome { label, line: format!("cannot write {}: {}", path.display(), e), ok: false, paste: None },
                    }
                }
                Method::Guided(where_) => Outcome {
                    label,
                    line: format!("paste the entry below in {}", where_),
                    // Nothing is written: the person does the step.
                    ok: false,
                    paste: Some(paste_text(&cmd)),
                },
            }
        }
    }
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

// Print the summary: one line per row, then each guided step's paste.
pub fn print_summary(outcomes: &[Outcome]) {
    let width = outcomes.iter().map(|o| o.label.chars().count()).max().unwrap_or(0);
    for o in outcomes {
        println!("  {:w$}  {}", o.label, o.line, w = width);
    }
    for o in outcomes.iter().filter(|o| o.paste.is_some()) {
        println!("\n{}: paste this\n{}", o.label, o.paste.as_deref().unwrap_or(""));
    }
}

// The checklist on a terminal: Some(the chosen rows), None when the person quits.
// One enabled row is taken without asking. Off a terminal every row that needs no
// consent is taken. Mirrors docs/frontends/cli.md#ide-agents.
pub fn choose(items: &[Item], heading: &str, tty: bool, one_host: bool) -> Option<Vec<Item>> {
    let enabled: Vec<bool> = items.iter().map(|i| i.enabled()).collect();
    let usable: Vec<&Item> = items.iter().filter(|i| i.enabled()).collect();
    if usable.is_empty() {
        return Some(Vec::new());
    }
    if usable.len() == 1 {
        let only = usable[0].clone();
        println!("jazyk: setting up {}, the only option", only.row(one_host).name);
        return Some(vec![only]);
    }
    if !tty {
        return Some(usable.into_iter().filter(|i| !i.needs_consent()).cloned().collect());
    }
    let rows: Vec<crate::menu::Row> = items.iter().map(|i| i.row(one_host)).collect();
    let ticked = crate::menu::check(heading, &rows, &enabled)?;
    Some(items.iter().zip(ticked).filter(|(i, t)| *t && i.enabled()).map(|(i, _)| i.clone()).collect())
}

#[cfg(test)]
mod tests {
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
}
