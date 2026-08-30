// The slash command catalog: one list, served by the IDE proxy and the GUI pane
// alike, plus the implementations that do not depend on which frontend is asking.
// Mirrors docs/frontends/acp.md#slash-commands.
use crate::project::Project;

pub struct Command {
    pub name: &'static str,
    pub description: &'static str,
    // The hint an editor shows for a command that takes arguments.
    pub hint: Option<&'static str>,
    // Whether the command needs a project to mean anything.
    pub needs_project: bool,
}

pub const COMMANDS: [Command; 15] = [
    Command {
        name: "help",
        description: "what jazyk is, and what these commands do",
        hint: None,
        needs_project: false,
    },
    Command {
        name: "init",
        description: "set this project up, and say what is still unanswered",
        hint: None,
        needs_project: false,
    },
    Command {
        name: "config",
        description: "show the project's settings, or change one",
        hint: Some("key value, e.g. llm.model qwen3.8:27b-mlx"),
        needs_project: true,
    },
    Command {
        name: "model",
        description: "list the endpoint's models, or pin one for this project",
        hint: Some("name, e.g. qwen3.8:27b-mlx"),
        needs_project: true,
    },
    Command {
        name: "agent",
        description: "list the agents jazyk can drive, or switch to one",
        hint: Some("name, e.g. embedded, claude, codex, opencode"),
        needs_project: true,
    },
    Command {
        name: "status",
        description: "summarize the last build",
        hint: None,
        needs_project: true,
    },
    Command {
        name: "board",
        description: "the goal board as the scheduler would derive it now",
        hint: None,
        needs_project: true,
    },
    Command {
        name: "preview",
        description: "the next session's prompt, exactly as the model would receive it",
        hint: Some("goal or target, e.g. ent:order"),
        needs_project: true,
    },
    Command {
        name: "explain",
        description: "why a goal exists, or what a change to a target would open",
        hint: Some("goal or target, e.g. g:review-entity:ent:order"),
        needs_project: true,
    },
    Command {
        name: "ripple",
        description: "walk a change's cascade through the journal",
        hint: Some("target, generation, or doc; --back walks upstream"),
        needs_project: true,
    },
    Command {
        name: "questions",
        description: "list the standing questions on open findings",
        hint: None,
        needs_project: true,
    },
    Command {
        name: "compile",
        description: "reconcile the graph with the documents",
        hint: None,
        needs_project: true,
    },
    Command {
        name: "generate",
        description: "bind and generate the deliverable",
        hint: None,
        needs_project: true,
    },
    Command {
        name: "verify",
        description: "run verification over the ledger",
        hint: None,
        needs_project: true,
    },
    Command {
        name: "release",
        description: "approve pending changes in manual mode",
        hint: None,
        needs_project: true,
    },
];

// What this session offers. Outside a project only the two commands that can act
// there. Mirrors docs/frontends/acp.md#slash-commands.
pub fn available(in_project: bool) -> impl Iterator<Item = &'static Command> {
    COMMANDS
        .iter()
        .filter(move |c| in_project || !c.needs_project)
}

// Whether a typed word is one of ours. Takes the leading slash as typed.
pub fn is_command(word: &str, in_project: bool) -> bool {
    let name = word.trim_start_matches('/');
    available(in_project).any(|c| c.name == name)
}

// The command word and the rest of the line, for a prompt that starts with one.
pub fn split<'a>(text: &'a str, in_project: bool) -> Option<(&'static str, &'a str)> {
    let text = text.trim();
    let (word, rest) = match text.split_once(char::is_whitespace) {
        Some((w, r)) => (w, r.trim()),
        None => (text, ""),
    };
    let name = word.trim_start_matches('/');
    available(in_project)
        .find(|c| c.name == name)
        .map(|c| (c.name, rest))
}

pub fn help_text(in_project: bool) -> String {
    let mut s = String::from(
        "Jazyk is a natural language compiler. The documents under `docs/` are the source: \
         jazyk reconciles them into a graph of entities and requirements, and the graph drives \
         generation and verification. You edit prose; jazyk keeps the rest in step.\n\n",
    );
    if !in_project {
        s.push_str("This directory is not a jazyk project yet.\n\n");
    }
    s.push_str("Commands:\n");
    for c in available(in_project) {
        let args = c.hint.map(|h| format!(" <{}>", h)).unwrap_or_default();
        s.push_str(&format!("  /{}{}\n      {}\n", c.name, args, c.description));
    }
    s.push_str("\nAnything that is not a command goes to the agent as conversation.");
    s
}

// The settings that matter, their effective values, and whether the project states
// them or inherits them. Mirrors docs/frontends/acp.md#slash-commands.
pub fn config_text(proj: &Project, llm: &crate::llm::Llm) -> String {
    let global_llm = crate::project::load_global_llm();
    let global_acp = crate::project::load_global_acp();
    // Where a value actually came from, walked in the same order the resolver walks:
    // the environment wins over the project file, which wins over the global config.
    // Reading the key out of jazyk.toml would name the wrong source whenever an
    // environment variable is set over it.
    let from = |env_var: &str, in_project: bool, in_global: bool| -> &'static str {
        if !env_var.is_empty() && std::env::var(env_var).is_ok() {
            "env"
        } else if in_project {
            "project"
        } else if in_global {
            "global"
        } else {
            "default"
        }
    };
    let agent =
        crate::acp::config::resolve_acp(None, &proj.acp, &crate::project::load_global_acp(), |n| {
            std::env::var(n).ok()
        })
        .map(|a| a.name)
        .unwrap_or_else(|e| format!("unresolved ({})", e));
    let gs = crate::gen::GenSettings::resolve(proj);
    let toml = std::fs::read_to_string(proj.root.join("jazyk.toml")).unwrap_or_default();
    let states = |key: &str| toml.contains(key);
    let rows: Vec<(&str, String, &str)> = vec![
        (
            "acp.agent",
            agent,
            from(
                "JAZYK_ACP_AGENT",
                proj.acp.agent.is_some(),
                global_acp.agent.is_some(),
            ),
        ),
        (
            "llm.model",
            llm.model.clone(),
            from(
                "JAZYK_MODEL",
                proj.llm.model.is_some(),
                global_llm.model.is_some(),
            ),
        ),
        (
            "llm.base_url",
            llm.base_url.clone(),
            from(
                "JAZYK_LLM_BASE_URL",
                proj.llm.base_url.is_some(),
                global_llm.base_url.is_some(),
            ),
        ),
        (
            "workflow.compile",
            proj.workflow.compile.clone(),
            from("", states("compile"), false),
        ),
        (
            "workflow.generate",
            proj.workflow.generate.clone(),
            from("", states("generate"), false),
        ),
        (
            "workflow.worker",
            proj.workflow.worker.clone(),
            from("", states("worker"), false),
        ),
        (
            "gen.deliverable",
            gs.deliverable.display().to_string(),
            from("", proj.gen_deliverable.is_some(), false),
        ),
        (
            "gen.worker",
            gs.worker.clone(),
            from("", proj.gen_worker.is_some(), false),
        ),
    ];
    let mut s = format!("{}\n\n", proj.root.display());
    for (k, v, src) in rows {
        s.push_str(&format!("  {:20} {:<40} [{}]\n", k, v, src));
    }
    s.push_str(&format!(
        "\n  docs glob            {}\n",
        proj.docs_glob.join(", ")
    ));
    s.push_str(&format!(
        "  roots                {}\n",
        proj.roots.join(", ")
    ));
    s.push_str(
        "\nChange one with `/config <key> <value>`, which edits jazyk.toml in place.\n\
         The bracket is where the value came from: `env` beats `project`, which beats \
         `global` (~/.jazyk/config.toml), which beats `default`. Setting a key the \
         environment already fixes writes the file but changes nothing here.\n\
         In an IDE the model is also a session setting: pick it from the agent's own model \
         selector, which changes this session only.",
    );
    s
}

// The keys `/config` will edit, the same set the chat tool takes; `executors.<kind>`
// and `executors.<class>` route sessions of that kind or class to a profile.
// Mirrors docs/frontends/acp.md#project-tools.
pub const CONFIG_KEYS: [&str; 8] = [
    "acp.agent",
    "llm.model",
    "llm.base_url",
    "workflow.compile",
    "workflow.generate",
    "workflow.worker",
    "gen.deliverable",
    "gen.worker",
];

// An editable key: one of CONFIG_KEYS, or an [executors] override per goal kind or
// class. Mirrors docs/compiler/project-settings.md#executors.
fn editable_key(key: &str) -> bool {
    if CONFIG_KEYS.contains(&key) {
        return true;
    }
    key.strip_prefix("executors.")
        .is_some_and(|k| crate::project::EXECUTOR_KEYS.contains(&k))
}

pub fn config_set(proj: &Project, args: &str) -> String {
    let (key, value) = match args.split_once(char::is_whitespace) {
        Some((k, v)) => (k.trim(), v.trim()),
        None => (args.trim(), ""),
    };
    if !editable_key(key) {
        return format!(
            "`{}` is not one of the editable keys:\n  {}\n  executors.<kind|class> (kinds and classes: {})",
            key,
            CONFIG_KEYS.join("\n  "),
            crate::project::EXECUTOR_KEYS.join(", ")
        );
    }
    if value.is_empty() {
        return format!("`/config {} <value>` needs a value", key);
    }
    let path = proj.root.join("jazyk.toml");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let (section, k) = key.split_once('.').unwrap_or(("", key));
    let full = crate::mcp::toml_set(&old, section, k, value);
    match std::fs::write(&path, full) {
        Ok(()) => format!("{} = {}\n\njazyk.toml updated.", key, value),
        Err(e) => format!("cannot write {}: {}", path.display(), e),
    }
}

// The board as the scheduler would derive it now: the summary line, one line per
// goal, and the batches. Shared by the proxy, the GUI pane, and the CLI.
// Mirrors docs/frontends/acp.md#slash-commands.
pub fn board_text(proj: &Project, out: &std::path::Path) -> String {
    let board = crate::board::Board::compute(proj, out);
    let mut s = format!("{}\n", board.summary_line());
    let rendered = board.render();
    if rendered.is_empty() {
        s.push_str(&format!("verdict: {}\n", board.verdict()));
    } else {
        s.push_str(&rendered);
        s.push('\n');
    }
    if !board.batches.is_empty() {
        s.push_str("batches:\n");
        for b in &board.batches {
            s.push_str(&format!(
                "  {}  {} goal(s), {}\n",
                b.id,
                b.goals.len(),
                b.locality
            ));
        }
    }
    s
}

// The next session's prompt, exactly as the model would receive it; with a goal or
// target, the batch that goal would join. Mirrors docs/compiler/sessions.md#preview.
pub fn preview_text(proj: &Project, out: &std::path::Path, target: &str) -> String {
    let mut store = crate::store::Store::load(out);
    let (parsed, _) = crate::reconcile::parse_all(proj);
    store.sync_docs(&parsed);
    let control = crate::control::Control::load(proj, out);
    let board = crate::board::Board::derive(&store, proj, &control);
    let target = target.trim();
    let batch = if target.is_empty() {
        board.batches.first()
    } else {
        board.batches.iter().find(|b| {
            b.id == target
                || b.goals
                    .iter()
                    .any(|id| id == target || board.goal(id).is_some_and(|g| g.target == target))
        })
    };
    let Some(batch) = batch else {
        return if target.is_empty() {
            "no ready batch; /board says why".to_string()
        } else {
            format!("no ready batch holds `{}`; /board says why", target)
        };
    };
    let goals: Vec<crate::model::Goal> = batch
        .goals
        .iter()
        .filter_map(|id| board.goal(id))
        .cloned()
        .collect();
    let (loaded, skills) = crate::session::initial_loaded(&store, &goals);
    let mut pb = crate::session::ProjectBlock::compute(&store, &goals, &control.compile);
    pb.batch = batch.id.clone();
    crate::session::session_prompt(&store, &goals, &loaded, &skills, &pb)
}

// Why a goal exists (its change, cause, readiness, blockers), or what a change to a
// target would open. Mirrors docs/frontends/cli.md#jazyk-explain.
pub fn explain_text(proj: &Project, out: &std::path::Path, target: &str) -> String {
    let target = target.trim();
    if target.is_empty() {
        return "explain what? pass a goal id (g:...) or a target (an id, a section, a document)"
            .to_string();
    }
    let mut store = crate::store::Store::load(out);
    let (parsed, _) = crate::reconcile::parse_all(proj);
    store.sync_docs(&parsed);
    let control = crate::control::Control::load(proj, out);
    let board = crate::board::Board::derive(&store, proj, &control);
    board
        .explain(&store, target)
        .unwrap_or_else(|| format!("`{}` names no goal and no known target", target))
}

// Walk a change's cascade through the journal, one line per entry.
// Mirrors docs/frontends/cli.md#jazyk-ripple.
pub fn ripple_text(_proj: &Project, out: &std::path::Path, args: &str) -> String {
    let mut back = false;
    let mut target = String::new();
    for w in args.split_whitespace() {
        if w == "--back" {
            back = true;
        } else {
            target = w.to_string();
        }
    }
    if target.is_empty() {
        return "ripple what? pass a target, a generation (g412), or a document (--back walks upstream)"
            .to_string();
    }
    let store = crate::store::Store::load(out);
    match crate::reconcile::ripple(&store, &target, back) {
        Some(tree) => crate::reconcile::render_ripple(&tree),
        None => format!(
            "nothing to walk from `{}`; the journal holds no entry touching it",
            target
        ),
    }
}

// The endpoint's models with the current one marked.
// Mirrors docs/frontends/acp.md#slash-commands.
pub fn model_text(llm: &crate::llm::Llm) -> String {
    let mut s = format!("Models at {}:\n", llm.base_url);
    for m in llm.list_models() {
        let mark = if m == llm.model { "  (current)" } else { "" };
        s.push_str(&format!("  {}{}\n", m, mark));
    }
    s.push_str(
        "\nPin one with `/model <name>`: it lands in jazyk.toml, and where the agent \
         takes a `model` config option it applies to this session too.",
    );
    s
}

// Pinning a model is a config edit plus honesty about what the edit cannot reach.
pub fn model_set(proj: &Project, llm: &crate::llm::Llm, name: &str) -> String {
    let mut s = config_set(proj, &format!("llm.model {}", name));
    if !llm.list_models().iter().any(|m| m == name) {
        s.push_str(&format!(
            "\n\nNote: {} does not list `{}`. The name is written as given; prompting \
             fails if the endpoint does not know it.",
            llm.base_url, name
        ));
    }
    if std::env::var("JAZYK_MODEL").is_ok() {
        s.push_str("\n\nJAZYK_MODEL is set in this environment and wins over the file.");
    }
    s
}

// Every agent jazyk can drive: the built-ins, then the profiles the project or the
// global config defines. Mirrors docs/frontends/acp.md#slash-commands.
pub fn known_agents(proj: &Project) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = crate::cli::ACP_AGENTS
        .iter()
        .map(|(name, label, _, _)| (name.to_string(), label.to_string()))
        .collect();
    let global = crate::project::load_global_acp();
    for (name, p) in proj.acp.agents.iter().chain(global.agents.iter()) {
        if !v.iter().any(|(n, _)| n == name) {
            v.push((
                name.clone(),
                format!("configured: {} {}", p.command, p.args.join(" ")),
            ));
        }
    }
    v
}

pub fn agent_text(proj: &Project) -> String {
    let current =
        crate::acp::config::resolve_acp(None, &proj.acp, &crate::project::load_global_acp(), |n| {
            std::env::var(n).ok()
        })
        .map(|a| a.name)
        .unwrap_or_default();
    let mut s = String::from("Agents jazyk can drive:\n");
    for (name, label) in known_agents(proj) {
        let mark = if name == current { "  (current)" } else { "" };
        s.push_str(&format!("  {:12} {}{}\n", name, label, mark));
    }
    s.push_str(
        "\nSwitch with `/agent <name>`. A custom agent is an `[acp.agents.<name>]` \
         entry in jazyk.toml with `command` and `args`.",
    );
    s
}

pub fn agent_set(proj: &Project, name: &str) -> String {
    let known = known_agents(proj);
    if !known.iter().any(|(n, _)| n == name) {
        return format!(
            "`{}` is not an agent jazyk knows:\n{}\nDefine one with an `[acp.agents.{}]` \
             entry in jazyk.toml (`command`, `args`), then run `/agent {}` again.",
            name,
            known
                .iter()
                .map(|(n, l)| format!("  {:12} {}\n", n, l))
                .collect::<String>(),
            name,
            name
        );
    }
    let mut s = config_set(proj, &format!("acp.agent {}", name));
    s.push_str(
        "\n\nThe switch takes effect when the jazyk agent restarts: reopen the window, \
         or restart the agent in the IDE.",
    );
    s
}

// What a fresh project still needs answered, as prose plus the command that answers
// it. A chat turn cannot stop and prompt, so the walkthrough is a list of the next
// moves rather than a questionnaire. Mirrors docs/frontends/acp.md#slash-commands.
pub fn init_next_steps(proj: &Project, llm: &crate::llm::Llm) -> String {
    let mut s = String::new();
    let agent =
        crate::acp::config::resolve_acp(None, &proj.acp, &crate::project::load_global_acp(), |n| {
            std::env::var(n).ok()
        });
    let toml = std::fs::read_to_string(proj.root.join("jazyk.toml")).unwrap_or_default();
    let states_agent = toml.contains("[acp]");
    match &agent {
        Ok(a) if a.name == crate::acp::config::EMBEDDED && !states_agent => {
            s.push_str(&format!(
                "- Agent: the built-in one, prompting `{}` at {}. Another agent takes over with \
                 `/agent <name>`.\n",
                llm.model, llm.base_url
            ));
        }
        Ok(a) => s.push_str(&format!("- Agent: {}.\n", a.name)),
        Err(e) => s.push_str(&format!(
            "- Agent: unresolved ({}). Set `/config acp.agent <name>`.\n",
            e
        )),
    }
    if !toml.contains("model") {
        s.push_str(&format!(
            "- Model: `{}`, inherited rather than stated here. `/model <name>` pins it \
             to this project.\n",
            llm.model
        ));
    }
    let root_doc = proj.roots.first().map(|r| proj.root.join(r));
    let placeholder = root_doc
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| t.contains("TODO"))
        .unwrap_or(false);
    if placeholder {
        s.push_str(&format!(
            "- The root document ({}) is still the placeholder. Describe what you are building \
             there; that prose is the source code.\n",
            proj.roots
                .first()
                .map(|s| s.as_str())
                .unwrap_or("docs/README.md")
        ));
    }
    s.push_str("\nWhen the documents say something, run `/compile`.");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    // Outside a project only the commands that can act there are offered, and the
    // parser agrees with the catalog. Mirrors docs/frontends/acp.md#slash-commands.
    #[test]
    fn the_catalog_follows_the_directory() {
        let bare: Vec<&str> = available(false).map(|c| c.name).collect();
        assert_eq!(bare, vec!["help", "init"]);
        assert_eq!(available(true).count(), COMMANDS.len());

        assert_eq!(
            split("/config llm.model qwen3", true),
            Some(("config", "llm.model qwen3"))
        );
        assert_eq!(split("/status", true), Some(("status", "")));
        assert_eq!(
            split("/status", false),
            None,
            "no build command outside a project"
        );
        assert_eq!(split("just talking", true), None);
        assert!(is_command("/help", false));

        // Every command reaches the person who typed it: help names them all.
        let help = help_text(true);
        for c in COMMANDS {
            assert!(
                help.contains(&format!("/{}", c.name)),
                "{} missing from help",
                c.name
            );
        }
        assert!(help_text(false).contains("not a jazyk project"));
    }

    // A settings edit is a minimal edit to jazyk.toml, and an unknown key is refused
    // with the list rather than written anywhere.
    #[test]
    fn config_set_edits_known_keys_only() {
        let dir = std::env::temp_dir().join(format!("jazyk-cmd-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("jazyk.toml"),
            "[docs]\nglob = [\"docs/**/*.md\"]\n",
        )
        .unwrap();
        let proj = Project::load(&dir);

        let out = config_set(&proj, "llm.model qwen3.8:27b-mlx");
        assert!(out.contains("updated"), "{}", out);
        let toml = std::fs::read_to_string(dir.join("jazyk.toml")).unwrap();
        assert!(toml.contains("model = \"qwen3.8:27b-mlx\""), "{}", toml);
        assert!(
            toml.contains("glob"),
            "the existing settings survive: {}",
            toml
        );

        let refused = config_set(&proj, "llm.api_key sk-secret");
        assert!(
            refused.contains("not one of the editable keys"),
            "{}",
            refused
        );
        let toml = std::fs::read_to_string(dir.join("jazyk.toml")).unwrap();
        assert!(!toml.contains("sk-secret"), "a refused key writes nothing");
        std::fs::remove_dir_all(&dir).ok();
    }
}
