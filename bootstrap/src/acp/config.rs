// Agent profile resolution: which downstream ACP agent performs AI work. Pure
// configuration; nothing agent-specific lives in code.
// Mirrors docs/compiler/project-settings.md#acp and docs/frontends/acp.md#agents.
use crate::project::AcpSettings;

// The resolved agent: what the host spawns.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedAgent {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub serve_files: bool,
}

pub const EMBEDDED: &str = "embedded";

// Per-field precedence, same ladder as resolve_llm: CLI flag → JAZYK_ACP_AGENT →
// project [acp] → global config → built-in default `embedded`. `env` is injected for
// testability.
pub fn resolve_acp(
    flag: Option<&str>,
    proj: &AcpSettings,
    global: &AcpSettings,
    env: impl Fn(&str) -> Option<String>,
) -> Result<ResolvedAgent, String> {
    let name = flag
        .map(|s| s.to_string())
        .or_else(|| env("JAZYK_ACP_AGENT"))
        .or_else(|| proj.agent.clone())
        .or_else(|| global.agent.clone())
        .unwrap_or_else(|| EMBEDDED.to_string());
    profile_named(&name, proj, global)
}

// The executor for one goal batch: the profile a session of this kind runs on.
// Precedence, first match wins: the --agent flag, JAZYK_ACP_AGENT, the project
// [executors].<kind>, the global [executors].<kind>, the project [executors].<class>,
// the global [executors].<class>, then the [acp] agent ladder.
// Mirrors docs/compiler/control-plane.md#executors.
#[allow(clippy::too_many_arguments)]
pub fn resolve_executor(
    flag: Option<&str>,
    kind: &str,
    class: &str,
    proj: &AcpSettings,
    proj_execs: &crate::project::Executors,
    global: &AcpSettings,
    global_execs: &crate::project::Executors,
    env: impl Fn(&str) -> Option<String>,
) -> Result<ResolvedAgent, String> {
    // The flag and the variable name one agent for the whole run and outrank the table.
    let named = flag
        .map(|s| s.to_string())
        .or_else(|| env("JAZYK_ACP_AGENT"))
        .or_else(|| {
            // Per key, the project table shadows the global one; the kind outranks
            // the class.
            proj_execs
                .by_kind
                .get(kind)
                .or_else(|| global_execs.by_kind.get(kind))
                .or_else(|| proj_execs.by_kind.get(class))
                .or_else(|| global_execs.by_kind.get(class))
                .cloned()
        });
    match named {
        Some(name) => profile_named(&name, proj, global),
        None => resolve_acp(None, proj, global, |_| None),
    }
}

// One named profile from the project or global [acp.agents] tables, or a built-in
// one. A table entry shadows the built-in of the same name.
fn profile_named(
    name: &str,
    proj: &AcpSettings,
    global: &AcpSettings,
) -> Result<ResolvedAgent, String> {
    let profile = proj.agents.get(name).or_else(|| global.agents.get(name));
    match profile {
        Some(p) if !p.command.is_empty() => Ok(ResolvedAgent {
            name: name.to_string(),
            command: p.command.clone(),
            args: p.args.clone(),
            env: p.env.clone(),
            serve_files: p.serve_files,
        }),
        Some(_) => Err(format!("agent profile `{}` has no command", name)),
        None => builtin_profile(name).ok_or_else(|| {
            format!(
                "unknown agent profile `{}`; define [acp.agents.{}] in jazyk.toml or the global config",
                name, name
            )
        }),
    }
}

// The agents jazyk can drive with no profile written: name, label, command, args,
// extra environment. The adapters are pinned to the versions whose steering contract
// the host knows (docs/frontends/acp.md#steering-active-turns). Claude Code refuses to
// start inside another Claude Code session, so its profile clears `CLAUDECODE`.
// Mirrors docs/frontends/acp.md#agents.
pub const BUILTIN_AGENTS: &[(&str, &str, &str, &[&str], &[(&str, &str)])] = &[
    (
        EMBEDDED,
        "Embedded (jazyk's own agent, over your LLM endpoint)",
        "",
        &[],
        &[],
    ),
    (
        "claude",
        "Claude Code",
        "npx",
        &["--yes", "@agentclientprotocol/claude-agent-acp@0.99.0"],
        &[("CLAUDECODE", "")],
    ),
    (
        "codex",
        "Codex",
        "npx",
        &["--yes", "@agentclientprotocol/codex-acp@2.4.0"],
        &[],
    ),
    ("opencode", "OpenCode", "opencode", &["acp"], &[]),
];

// Built-in profiles an `[acp.agents.<name>]` table replaces, one line each naming the
// file that does it. A stale override (an old adapter, a forced model) is invisible
// otherwise. Mirrors docs/frontends/acp.md#agents.
pub fn shadow_notes(
    project: &crate::project::AcpSettings,
    project_file: Option<&std::path::Path>,
    global: &crate::project::AcpSettings,
) -> Vec<String> {
    let global_file = crate::sys::env_var("HOME")
        .map(|h| {
            std::path::PathBuf::from(h)
                .join(".jazyk/config.toml")
                .display()
                .to_string()
        })
        .unwrap_or_else(|_| "the global config".to_string());
    let mut out = Vec::new();
    for (name, ..) in BUILTIN_AGENTS.iter().filter(|a| a.0 != EMBEDDED) {
        let file = if project.agents.contains_key(*name) {
            project_file
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "the project's jazyk.toml".to_string())
        } else if global.agents.contains_key(*name) {
            global_file.clone()
        } else {
            continue;
        };
        out.push(format!(
            "note: [acp.agents.{}] in {} replaces the built-in `{}` profile; remove it to use the built-in one",
            name, file, name
        ));
    }
    out
}

// A built-in profile by name. Mirrors docs/frontends/acp.md#agents.
pub fn builtin_profile(name: &str) -> Option<ResolvedAgent> {
    if name == EMBEDDED {
        return Some(embedded_profile());
    }
    BUILTIN_AGENTS
        .iter()
        .find(|a| a.0 == name)
        .map(|(name, _, command, args, env)| ResolvedAgent {
            name: name.to_string(),
            command: command.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            serve_files: false,
        })
}

// The built-in profile: this binary serving `jazyk agent` over stdio. It has no
// editor of its own, so jazyk serves the file tools into its sessions.
pub fn embedded_profile() -> ResolvedAgent {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "jazyk".to_string());
    ResolvedAgent {
        name: EMBEDDED.to_string(),
        command: exe,
        args: vec!["agent".to_string()],
        env: Vec::new(),
        serve_files: true,
    }
}

// The transport of the serving injected into a session.
// Mirrors docs/frontends/acp.md#worker-sessions and docs/frontends/mcp.md#mcp-over-http.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum McpTransport {
    // The default: the entry names `jazyk mcp` and the agent spawns it.
    Stdio,
    // The agent advertised `mcpCapabilities.http`: jazyk serves the session's toolset
    // over MCP streamable HTTP on a loopback port for the session's lifetime.
    Http,
}

// The choice from what the agent's initialize reply advertised and the run's pin
// (`JAZYK_ACP_MCP`, `stdio` or `http`; anything else means follow the reply).
// Protocol version 1 has no stdio flag, so an agent that advertises HTTP is taken at
// its word.
pub fn choose_transport(http_advertised: bool, pin: Option<&str>) -> McpTransport {
    match pin.map(|p| p.trim().to_ascii_lowercase()).as_deref() {
        Some("stdio") => McpTransport::Stdio,
        Some("http") => McpTransport::Http,
        _ if http_advertised => McpTransport::Http,
        _ => McpTransport::Stdio,
    }
}

pub fn mcp_transport_pin() -> Option<String> {
    std::env::var("JAZYK_ACP_MCP")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

// Idle watchdog for worker sessions: a turn with no update for this long is cancelled.
// Mirrors docs/compiler/project-settings.md#environment-tuning.
pub fn idle_timeout() -> std::time::Duration {
    let secs = std::env::var("JAZYK_ACP_IDLE_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1800)
        .max(30);
    std::time::Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {

    // A table named after a built-in is reported with its file; others are not.
    #[test]
    fn shadowed_builtins_name_their_file() {
        let mut project = crate::project::AcpSettings::default();
        let mut global = crate::project::AcpSettings::default();
        let profile = |cmd: &str| crate::project::AcpAgentProfile {
            command: cmd.into(),
            ..Default::default()
        };
        global.agents.insert("codex".into(), profile("npx"));
        global.agents.insert("mine".into(), profile("x"));
        project.agents.insert("claude".into(), profile("npx"));
        let notes = shadow_notes(
            &project,
            Some(std::path::Path::new("/p/jazyk.toml")),
            &global,
        );
        assert_eq!(notes.len(), 2, "{:?}", notes);
        assert!(notes[0].contains("[acp.agents.claude] in /p/jazyk.toml"));
        assert!(notes[1].contains("[acp.agents.codex]") && notes[1].contains(".jazyk/config.toml"));
    }

    use super::*;
    use crate::project::AcpAgentProfile;
    use std::collections::BTreeMap;

    fn settings(agent: Option<&str>, profiles: &[(&str, &str)]) -> AcpSettings {
        let mut agents = BTreeMap::new();
        for (name, cmd) in profiles {
            agents.insert(
                name.to_string(),
                AcpAgentProfile {
                    command: cmd.to_string(),
                    ..Default::default()
                },
            );
        }
        AcpSettings {
            agent: agent.map(|s| s.to_string()),
            agents,
        }
    }

    #[test]
    fn default_is_the_embedded_agent() {
        let r = resolve_acp(
            None,
            &AcpSettings::default(),
            &AcpSettings::default(),
            |_| None,
        )
        .unwrap();
        assert_eq!(r.name, "embedded");
        assert_eq!(r.args, vec!["agent"]);
        assert!(r.serve_files);
    }

    #[test]
    fn precedence_flag_env_project_global() {
        let proj = settings(
            Some("proj"),
            &[("proj", "p"), ("envy", "e"), ("flagged", "f")],
        );
        let global = settings(Some("glob"), &[("glob", "g")]);
        let with_env = |r: Result<ResolvedAgent, String>| r.unwrap().name;
        assert_eq!(
            with_env(resolve_acp(Some("flagged"), &proj, &global, |_| Some(
                "envy".into()
            ))),
            "flagged"
        );
        assert_eq!(
            with_env(resolve_acp(None, &proj, &global, |_| Some("envy".into()))),
            "envy"
        );
        assert_eq!(
            with_env(resolve_acp(None, &proj, &global, |_| None)),
            "proj"
        );
        assert_eq!(
            with_env(resolve_acp(None, &AcpSettings::default(), &global, |_| {
                None
            })),
            "glob"
        );
    }

    #[test]
    fn a_project_profile_shadows_a_global_one() {
        let proj = settings(None, &[("shared", "project-cmd")]);
        let global = settings(Some("shared"), &[("shared", "global-cmd")]);
        let r = resolve_acp(None, &proj, &global, |_| None).unwrap();
        assert_eq!(r.command, "project-cmd");
    }

    // A built-in agent resolves by name alone; a table entry of the same name shadows
    // it. Mirrors docs/frontends/acp.md#agents.
    #[test]
    fn builtin_agents_resolve_without_a_profile() {
        let none = settings(None, &[]);
        let claude = resolve_acp(Some("claude"), &none, &none, |_| None).unwrap();
        assert_eq!(claude.command, "npx");
        assert!(claude
            .args
            .iter()
            .any(|a| a == "@agentclientprotocol/claude-agent-acp@0.99.0"));
        assert!(claude
            .env
            .contains(&("CLAUDECODE".to_string(), String::new())));
        let opencode = resolve_acp(Some("opencode"), &none, &none, |_| None).unwrap();
        assert_eq!(
            (opencode.command.as_str(), opencode.args.clone()),
            ("opencode", vec!["acp".to_string()])
        );
        let own = settings(None, &[("codex", "my-codex")]);
        assert_eq!(
            resolve_acp(Some("codex"), &own, &none, |_| None)
                .unwrap()
                .command,
            "my-codex"
        );
    }

    // Executor precedence: the kind's key over the class's key over the [acp] agent,
    // and the flag over everything. Mirrors docs/compiler/control-plane.md#executors.
    #[test]
    fn resolve_executor_prefers_kind_over_class_over_agent() {
        use crate::project::Executors;
        use std::collections::BTreeMap;
        let proj = settings(
            Some("base"),
            &[
                ("base", "b"),
                ("classy", "c"),
                ("kindly", "k"),
                ("flagged", "f"),
            ],
        );
        let global = settings(None, &[]);
        let execs = |pairs: &[(&str, &str)]| Executors {
            by_kind: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
        };
        let none = Executors::default();
        let name = |r: Result<ResolvedAgent, String>| r.unwrap().name;
        let both = execs(&[("abstract-entity", "kindly"), ("gc", "classy")]);
        assert_eq!(
            name(resolve_executor(
                None,
                "abstract-entity",
                "gc",
                &proj,
                &both,
                &global,
                &none,
                |_| None
            )),
            "kindly"
        );
        let class_only = execs(&[("gc", "classy")]);
        assert_eq!(
            name(resolve_executor(
                None,
                "abstract-entity",
                "gc",
                &proj,
                &class_only,
                &global,
                &none,
                |_| None
            )),
            "classy"
        );
        assert_eq!(
            name(resolve_executor(
                None,
                "abstract-entity",
                "gc",
                &proj,
                &none,
                &global,
                &none,
                |_| None
            )),
            "base"
        );
        assert_eq!(
            name(resolve_executor(
                Some("flagged"),
                "abstract-entity",
                "gc",
                &proj,
                &both,
                &global,
                &none,
                |_| None
            )),
            "flagged"
        );
        // The global table fills a kind the project table lacks.
        assert_eq!(
            name(resolve_executor(
                None,
                "abstract-entity",
                "gc",
                &proj,
                &none,
                &global,
                &execs(&[("abstract-entity", "kindly")]),
                |_| None
            )),
            "kindly"
        );
    }

    // The transport follows the capability: HTTP when advertised, stdio otherwise,
    // and the pin outranks the reply either way.
    // Mirrors docs/frontends/acp.md#worker-sessions.
    #[test]
    fn transport_follows_the_advertised_capability_unless_pinned() {
        assert_eq!(choose_transport(false, None), McpTransport::Stdio);
        assert_eq!(choose_transport(true, None), McpTransport::Http);
        assert_eq!(choose_transport(true, Some("auto")), McpTransport::Http);
        assert_eq!(choose_transport(true, Some("stdio")), McpTransport::Stdio);
        assert_eq!(choose_transport(false, Some("http")), McpTransport::Http);
        assert_eq!(choose_transport(false, Some(" HTTP ")), McpTransport::Http);
    }

    #[test]
    fn unknown_profile_is_an_error_naming_the_repair() {
        let e = resolve_acp(
            Some("nope"),
            &AcpSettings::default(),
            &AcpSettings::default(),
            |_| None,
        )
        .unwrap_err();
        assert!(e.contains("acp.agents.nope"));
    }
}
