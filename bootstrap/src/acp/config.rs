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
    let profile = proj.agents.get(&name).or_else(|| global.agents.get(&name));
    match profile {
        Some(p) if !p.command.is_empty() => Ok(ResolvedAgent {
            name: name.clone(),
            command: p.command.clone(),
            args: p.args.clone(),
            env: p.env.clone(),
            serve_files: p.serve_files,
        }),
        Some(_) => Err(format!("agent profile `{}` has no command", name)),
        None if name == EMBEDDED => Ok(embedded_profile()),
        None => Err(format!(
            "unknown agent profile `{}`; define [acp.agents.{}] in jazyk.toml or the global config",
            name, name
        )),
    }
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

// Idle watchdog for worker sessions: a turn with no update for this long is cancelled.
// Mirrors docs/compiler/project-settings.md#environment-tuning.
pub fn idle_timeout() -> std::time::Duration {
    let secs = std::env::var("JAZYK_ACP_IDLE_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(600)
        .max(30);
    std::time::Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::AcpAgentProfile;
    use std::collections::BTreeMap;

    fn settings(agent: Option<&str>, profiles: &[(&str, &str)]) -> AcpSettings {
        let mut agents = BTreeMap::new();
        for (name, cmd) in profiles {
            agents.insert(
                name.to_string(),
                AcpAgentProfile { command: cmd.to_string(), ..Default::default() },
            );
        }
        AcpSettings { agent: agent.map(|s| s.to_string()), agents }
    }

    #[test]
    fn default_is_the_embedded_agent() {
        let r = resolve_acp(None, &AcpSettings::default(), &AcpSettings::default(), |_| None).unwrap();
        assert_eq!(r.name, "embedded");
        assert_eq!(r.args, vec!["agent"]);
        assert!(r.serve_files);
    }

    #[test]
    fn precedence_flag_env_project_global() {
        let proj = settings(Some("proj"), &[("proj", "p"), ("envy", "e"), ("flagged", "f")]);
        let global = settings(Some("glob"), &[("glob", "g")]);
        let with_env = |r: Result<ResolvedAgent, String>| r.unwrap().name;
        assert_eq!(
            with_env(resolve_acp(Some("flagged"), &proj, &global, |_| Some("envy".into()))),
            "flagged"
        );
        assert_eq!(with_env(resolve_acp(None, &proj, &global, |_| Some("envy".into()))), "envy");
        assert_eq!(with_env(resolve_acp(None, &proj, &global, |_| None)), "proj");
        assert_eq!(
            with_env(resolve_acp(None, &AcpSettings::default(), &global, |_| None)),
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

    #[test]
    fn unknown_profile_is_an_error_naming_the_repair() {
        let e = resolve_acp(Some("nope"), &AcpSettings::default(), &AcpSettings::default(), |_| None)
            .unwrap_err();
        assert!(e.contains("acp.agents.nope"));
    }
}
