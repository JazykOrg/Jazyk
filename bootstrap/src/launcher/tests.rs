// The command table: every command has a shape and a help, and the help lists
// exactly the options the shape honors, so `--help` never omits an accepted option.
// Mirrors docs/frontends/cli.md#help.
use super::*;

const COMMANDS: &[&str] = &[
    "init",
    "setup",
    "compile",
    "sets",
    "check",
    "watch",
    "decompile",
    "status",
    "preview",
    "explain",
    "ripple",
    "context",
    "query",
    "gen",
    "test",
    "codegen",
    "testgen",
    "docsgen",
    "agent",
    "lsp",
    "viewer",
    "gui",
    "mcp",
    "monitor",
    "release",
    "answer",
    "spend",
    "acp",
    "benchmark",
    "replay",
    "tasks",
    "runs",
    "run",
    "run-status",
    "run-log",
    "run-cancel",
];

fn every_command() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = COMMANDS.to_vec();
    v.extend(
        crate::start::CHOICES
            .iter()
            .map(|c| c.id)
            .filter(|id| *id != "gui"),
    );
    v
}

#[test]
fn every_command_has_a_shape_and_a_help_naming_its_options() {
    for cmd in every_command() {
        let (_, extra) = command_shape(cmd).unwrap_or_else(|| panic!("{cmd} has no shape"));
        let key = match cmd {
            "codegen" | "testgen" => "gen",
            c => c,
        };
        let help = cmd_usage(key).unwrap_or_else(|| panic!("{cmd} has no help"));
        assert!(
            help.starts_with(&format!("usage: jazyk {key}")),
            "{cmd}: {help}"
        );
        for option in extra.iter().filter(|o| **o != "--stdio") {
            assert!(
                help.contains(option),
                "`{cmd} --help` omits {option}: {help}"
            );
        }
    }
}

#[test]
fn unknown_names_have_no_shape_and_no_help() {
    for name in ["frobnicate", "", "--help", "Status", "run_status"] {
        assert!(command_shape(name).is_none(), "{name}");
        assert!(cmd_usage(name).is_none(), "{name}");
    }
}

// Every option a shape names is one the parser knows: a value option, a common
// option, or a flag the command line accepts (the help for it would otherwise
// advertise an option the parser rejects as unknown).
#[test]
fn every_shaped_option_is_a_known_option() {
    let top = top_usage();
    for cmd in every_command() {
        let (_, extra) = command_shape(cmd).unwrap();
        for option in extra.iter() {
            assert!(option.starts_with("--"), "{cmd}: {option}");
            assert!(
                !COMMON_OPTIONS.contains(option),
                "{cmd} repeats the common option {option}"
            );
        }
    }
    for option in COMMON_OPTIONS.iter().filter(|o| o.starts_with("--")) {
        assert!(top.contains(option), "the top-level help omits {option}");
    }
}
