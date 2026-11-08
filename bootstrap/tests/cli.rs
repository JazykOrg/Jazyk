// The command line as a user meets it: the built binary run in a scratch directory
// with a private HOME, no LLM endpoint, and a fake container engine on PATH. Only
// commands that answer before any work starts (help, usage errors) and the task
// commands' read paths run. Mirrors docs/frontends/cli.md#help.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
    "claude",
    "vscode",
    "cursor",
    "zed",
];

struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "jazyk-cli-{}-{}-{}",
            tag,
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("work")).unwrap();
        // A container engine that answers the readiness probe and nothing else.
        let engine = dir.join("bin/docker");
        std::fs::write(
            &engine,
            "#!/bin/sh\nif [ \"$1\" = info ]; then echo '{}'; exit 0; fi\necho \"fake engine refuses $1\" 1>&2\nexit 1\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
        Scratch { dir }
    }

    fn work(&self) -> PathBuf {
        self.dir.join("work")
    }

    fn project(&self) -> &Self {
        std::fs::create_dir_all(self.work().join(".jazyk")).unwrap();
        std::fs::write(
            self.work().join(".jazyk/jazyk.toml"),
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\n",
        )
        .unwrap();
        self
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_in(&self.work(), args)
    }

    fn run_in(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_jazyk"))
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .env("HOME", self.dir.join("home"))
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.dir.join("bin").display()),
            )
            .env("JAZYK_LLM_BASE_URL", "http://127.0.0.1:9/unreachable")
            .output()
            .unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).to_string()
}

#[track_caller]
fn usage_error(out: &Output, needle: &str) {
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains(needle), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty(), "{}", text(&out.stdout));
}

// Top-level help in each spelling exits 0 on stdout and lists the commands.
#[test]
fn top_level_help_lists_every_command() {
    let s = Scratch::new("top-help");
    for args in [&["--help"][..], &["-h"], &["help"]] {
        let out = s.run(args);
        assert_eq!(out.status.code(), Some(0), "{args:?}");
        let help = text(&out.stdout);
        assert!(help.contains("--help, -h"), "{help}");
        // Aliases, launcher choices (listed on one line), and the replay and task
        // commands (documented under their own pages) are not lines of their own.
        let unlisted = [
            "codegen",
            "testgen",
            "vscode",
            "cursor",
            "zed",
            "claude",
            "replay",
            "tasks",
            "runs",
            "run",
            "run-status",
            "run-log",
            "run-cancel",
        ];
        for cmd in COMMANDS.iter().filter(|c| !unlisted.contains(c)) {
            assert!(
                help.contains(cmd),
                "`{cmd}` missing from the top-level help"
            );
        }
    }
}

// Every command's `--help` (and `-h`, before or after the command) prints its usage
// and exits 0, in a project or not, without starting anything.
#[test]
fn every_command_prints_its_help_and_exits_zero() {
    let s = Scratch::new("cmd-help");
    for cmd in COMMANDS {
        for args in [vec![*cmd, "--help"], vec!["-h", *cmd]] {
            let out = s.run(&args);
            assert_eq!(
                out.status.code(),
                Some(0),
                "{args:?}: {}",
                text(&out.stderr)
            );
            let help = text(&out.stdout);
            let shown = match *cmd {
                "codegen" | "testgen" => "gen",
                c => c,
            };
            assert!(
                help.starts_with(&format!("usage: jazyk {shown}")),
                "{args:?}: {help}"
            );
        }
    }
    // Nothing was created by asking.
    assert_eq!(std::fs::read_dir(s.work()).unwrap().count(), 0);
}

// An unknown command is a usage error pointing at the top-level help, asked with
// --help or not.
#[test]
fn an_unknown_command_exits_two() {
    let s = Scratch::new("unknown-cmd");
    for args in [&["frobnicate"][..], &["frobnicate", "--help"]] {
        let out = s.run(args);
        usage_error(&out, "unknown command `frobnicate`");
        assert!(text(&out.stderr).contains("`jazyk --help`"));
    }
}

// Unknown options, options a command does not take, a value option without its
// value, and a number option with text are usage errors naming the command's help.
#[test]
fn bad_options_exit_two_naming_the_commands_help() {
    let s = Scratch::new("options");
    s.project();
    usage_error(&s.run(&["status", "--bogus"]), "unknown option `--bogus`");
    usage_error(&s.run(&["--bogus", "status"]), "unknown option `--bogus`");
    usage_error(&s.run(&["status", "-x"]), "unknown option `-x`");
    usage_error(
        &s.run(&["check", "--sessions", "0"]),
        "`check` does not take --sessions",
    );
    usage_error(&s.run(&["status", "--set"]), "--set takes a value");
    usage_error(
        &s.run(&["context", "--depth", "deep", "ent:x"]),
        "--depth takes a whole number, got `deep`",
    );
    usage_error(
        &s.run(&["gui", "--port", "-1"]),
        "--port takes a whole number",
    );
    let out = s.run(&["status", "--bogus"]);
    assert!(
        text(&out.stderr).contains("`jazyk status --help`"),
        "{}",
        text(&out.stderr)
    );
    // Bare `jazyk` with a bad option is a usage error too.
    usage_error(&s.run(&["--bogus"]), "unknown option `--bogus`");
}

// Arity: too many positional arguments is a usage error stating what the command
// takes; a required argument that is missing is one too.
#[test]
fn positional_arity_is_checked_per_command() {
    let s = Scratch::new("arity");
    s.project();
    usage_error(&s.run(&["status", "extra"]), "`status` takes no arguments");
    usage_error(
        &s.run(&["preview", "a", "b"]),
        "`preview` takes one argument (got `a`, `b`)",
    );
    usage_error(&s.run(&["spend", "a", "b", "c"]), "at most 2 arguments");
    usage_error(&s.run(&["tasks", "x"]), "`tasks` takes no arguments");
    usage_error(&s.run(&["run"]), "`run` takes a task id");
    usage_error(&s.run(&["run-status"]), "`run-status` takes a run id");
    usage_error(&s.run(&["run-log"]), "`run-log` takes a run id");
    usage_error(&s.run(&["run-cancel"]), "`run-cancel` takes a run id");
    usage_error(&s.run(&["run", "a", "b"]), "`run` takes one argument");
    usage_error(&s.run(&["query"]), "`query` takes the text");
    usage_error(&s.run(&["context"]), "`context` takes a target");
}

// The task commands: outside a project they are refused (1); inside one, `tasks`
// and `runs` print the runtime's JSON with the engine's readiness, and a run id
// that does not exist is refused (1), never a usage error.
#[test]
fn task_commands_answer_json_in_a_project_and_refuse_outside() {
    let s = Scratch::new("tasks");
    let outside = s.run(&["tasks"]);
    assert_eq!(outside.status.code(), Some(1), "{}", text(&outside.stderr));
    assert!(text(&outside.stderr).starts_with("jazyk: "));
    s.project();
    for cmd in ["tasks", "runs"] {
        let out = s.run(&[cmd]);
        assert_eq!(out.status.code(), Some(0), "{cmd}: {}", text(&out.stderr));
        let v: serde_json::Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{cmd}: {e}: {}", text(&out.stdout)));
        assert!(v.is_object(), "{v}");
        assert!(text(&out.stdout).contains("available"), "{v}");
    }
    for cmd in ["run-status", "run-log", "run-cancel"] {
        let out = s.run(&[cmd, "no-such-run"]);
        assert_eq!(out.status.code(), Some(1), "{cmd}: {}", text(&out.stderr));
        assert!(text(&out.stderr).starts_with("jazyk: "), "{cmd}");
    }
    let out = s.run(&["run", "no-such-task"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    // From a subfolder the project is found by walking up.
    std::fs::create_dir_all(s.work().join("docs/deep")).unwrap();
    let out = s.run_in(&s.work().join("docs/deep"), &["tasks"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
}
