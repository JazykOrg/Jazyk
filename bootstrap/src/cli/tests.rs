use super::*;
use crate::project::{GlobalLlm, LlmSettings};

// The task commands go through the launcher like every other command: help,
// arity, and unknown options are the launcher's (docs/frontends/cli.md#help).
#[test]
fn task_commands_have_shapes_and_help() {
    for (cmd, arity) in [
        ("tasks", 0),
        ("runs", 0),
        ("run", 1),
        ("run-status", 1),
        ("run-log", 1),
        ("run-cancel", 1),
    ] {
        let (n, extra) =
            crate::launcher::command_shape(cmd).unwrap_or_else(|| panic!("no shape for {}", cmd));
        assert_eq!(n, Some(arity), "{}", cmd);
        assert!(extra.is_empty(), "{}", cmd);
        let usage =
            crate::launcher::cmd_usage(cmd).unwrap_or_else(|| panic!("no help for {}", cmd));
        assert!(
            usage.starts_with(&format!("usage: jazyk {}", cmd)),
            "{}",
            usage
        );
        assert!(usage.contains("2 on a usage error"), "{}", usage);
    }
    let main = include_str!("../main.rs");
    assert!(
        !main.contains("task_runtime::cli"),
        "main.rs must not bypass the launcher"
    );
}

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
    for cmd in [
        "preview", "explain", "ripple", "acp", "monitor", "release", "answer",
    ] {
        assert!(usage.contains(cmd), "`{}` missing from the top usage", cmd);
    }
    assert!(usage.contains("goal-based"), "{}", usage);
    assert!(!usage.contains('\u{2014}'), "em dash in the top usage");
    // The groups come in the documented order: what a person types first,
    // what programs spawn last.
    let start = usage.find("start here:").expect("start here group");
    let ides = usage.find("run by IDEs and agents").expect("IDE group");
    assert!(start < ides, "{}", usage);
    for (line, _) in crate::launcher::COMMAND_GROUPS
        .iter()
        .flat_map(|(_, cs)| cs.iter())
    {
        let Some(cmd) = line.split_whitespace().next() else {
            continue; // bare `jazyk`, the launcher
        };
        if cmd == "<choice>" {
            // One line for every launcher choice; each is its own command.
            for c in crate::start::CHOICES.iter().filter(|c| c.id != "gui") {
                assert!(
                    crate::launcher::cmd_usage(c.id).is_some(),
                    "no help for `{}`",
                    c.id
                );
            }
            continue;
        }
        let (_, extra) =
            crate::launcher::command_shape(cmd).unwrap_or_else(|| panic!("no shape for `{}`", cmd));
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
