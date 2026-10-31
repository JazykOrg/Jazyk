use crate::{acp, benchmark, cli, docsgen, gen, lsp, mcp, render, replay, store};
use crate::vfs::VPathExt;
// Load a .env file by walking up from the current directory. Does not override existing env vars.
fn load_dotenv() {
    let mut dir = std::env::current_dir().ok();
    while let Some(d) = dir {
        let f = d.join(".env");
        if f.vexists() {
            if let Ok(content) = crate::vfs::read_to_string(&f) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some((k, v)) = line.split_once('=') {
                        let k = k.trim();
                        let v = v.trim().trim_matches('"');
                        if std::env::var(k).is_err() {
                            std::env::set_var(k, v);
                        }
                    }
                }
            }
            break;
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }
}

// The commands in four groups, what a person types most first and what programs
// spawn last, in the order docs/frontends/cli.md#help lists them.
pub(crate) const COMMAND_GROUPS: &[(&str, &[(&str, &str)])] = &[
    ("start here", &[
        ("", "the launcher: status, then pick a web UI, IDE, or coding agent to set up and start"),
        ("<choice> [--agent NAME]", "set up and start one launcher choice: vscode, cursor, zed, idea, phpstorm, webstorm, pycharm, goland, rustrover, clion, rider, rubymine, claude, codex, gemini, opencode, pi, droid"),
        ("init [--mcp AGENT] [--agent NAME] [--acp IDE]", "scaffold a project (jazyk.toml, docs/, deliverable/) here"),
        ("gui [--port N]", "local GUI: web app, API, events, LSP over WebSocket"),
        ("sets list|add|update|remove|preview", "manage independent document sets"),
        ("status [--set ID] [--json] [--costs]", "every set's state and board on one line each; with --set, that set in full"),
        ("compile [path...] [--sessions N]", "run one build: sessions resolve goals until the board converges"),
        ("benchmark [case...]", "grade the configured agent and model"),
    ]),
    ("actions on the project", &[
        ("check [path...]", "compile, exit non-zero on error diagnostics (CI)"),
        ("watch [path...]", "recompile on change, one line per goal"),
        ("gen [entity...]", "resolve the bind and generate goals into the deliverable (--force)"),
        ("test [target...]", "resolve the verify goals (--kind programmatic|llm, --list, --audit, --force)"),
        ("decompile [path...]", "draft docs describing what unclaimed code does"),
        ("docsgen", "render the requirements documents, entity cards, level and diagram pages"),
        ("release [compile|generate]", "approve gated goals in manual mode without running anything"),
        ("answer <diag|goal> [--option N | --text T]", "answer a prompted diagnostic; without a reply, list its options"),
        ("spend <generation> <tokens> [--cached N]", "record what an external agent's session billed on its journal entry"),
    ]),
    ("inspect", &[
        ("preview [goal|target|batch]", "the next session's prompt, exactly as the model receives it"),
        ("explain [goal|target]", "why a goal exists, or what a change to a target opens"),
        ("ripple [root] [--back]", "walk a change's cascade through the journal (id, g412, or doc)"),
        ("context <target>", "what the `load` tool renders (--depth N, --expand HANDLE)"),
        ("query <text> [--json]", "search entities"),
        ("viewer [--out FILE]", "render the graph to a self-contained HTML page"),
    ]),
    ("run by IDEs and agents, not by hand", &[
        ("mcp <toolsets>", "the MCP server: all, compile,generate,verify,runtime,decompile,benchmark,chat,graph"),
        ("acp [install --ide CLIENT]", "the IDE-facing ACP proxy; `install` registers it with an editor"),
        ("lsp", "language server over stdio (read-only; compile or watch rebuilds)"),
        ("agent", "the embedded ACP agent over stdio (spawned by the bridge)"),
        ("monitor [--json] [--once]", "print ready and blocked goals as the state changes; --once exits at the first"),
    ]),
];

pub(crate) fn top_usage() -> String {
    let mut s = String::new();
    s.push_str("jazyk: natural language compiler (goal-based)\n\n");
    s.push_str("usage: jazyk [command] [options]\n");
    let width = COMMAND_GROUPS
        .iter()
        .flat_map(|(_, cs)| cs.iter().map(|(u, _)| u.len()))
        .max()
        .unwrap_or(0)
        + 2;
    for (group, commands) in COMMAND_GROUPS {
        s.push_str(&format!("\n{}:\n", group));
        for (usage, what) in *commands {
            s.push_str(&format!("  jazyk {:<width$} {}\n", usage, what, width = width));
        }
    }
    s.push_str("\noptions: --agent NAME  --llm-base-url URL  --model M  --api-key K  --out DIR\n");
    s.push_str(
        "         --verbose, -v   the full session trace: prompts, payloads, the goal cascade\n",
    );
    s.push_str("         --quiet, -q     only the board summary, gc bursts, and the verdict\n");
    s.push_str("         --help, -h      print help and exit\n");
    s.push_str("\n`jazyk <command> --help` prints that command's arguments, options, and exit codes.\n");
    s
}

fn usage() -> ! {
    eprintln!("{}", top_usage());
    std::process::exit(2);
}

// A usage error: the message, then where the right form is written. Exit 2, the
// code every usage error carries. Mirrors docs/frontends/cli.md#help.
fn usage_error(cmd: &str, msg: &str) -> ! {
    if cmd.is_empty() || cmd_usage(cmd).is_none() {
        eprintln!("jazyk: {}; `jazyk --help` lists the commands", msg);
    } else {
        eprintln!(
            "jazyk: {}; `jazyk {} --help` shows its arguments and options",
            msg, cmd
        );
    }
    std::process::exit(2);
}

// The options every command accepts: the common options and the trace levels.
const COMMON_OPTIONS: &[&str] = &[
    "--set",
    "--help",
    "-h",
    "--out",
    "--llm-base-url",
    "--model",
    "--api-key",
    "--agent",
    "--verbose",
    "-v",
    "--quiet",
    "-q",
];

// Options that take a value; a bare one at the end of the line is a usage error.
const VALUE_OPTIONS: &[&str] = &[
    "--set", "--docs", "--source", "--roots", "--name",
    "--llm-base-url",
    "--model",
    "--api-key",
    "--out",
    "--depth",
    "--expand",
    "--kind",
    "--port",
    "--gui-url",
    "--mcp",
    "--agent",
    "--acp",
    "--ide",
    "--only",
    "--project",
    "--goal",
    "--sessions",
    "--option",
    "--text",
    "--build-token",
    "--cancel-file",
    "--edit-sink",
    "--cached",
];

// How many positional arguments a command takes (None: any number), and the
// options it honors beyond the common ones. A command's help lists exactly these.
// Mirrors docs/frontends/cli.md#help.
pub(crate) fn command_shape(cmd: &str) -> Option<(Option<usize>, &'static [&'static str])> {
    Some(match cmd {
        "init" => (Some(0), &["--mcp", "--acp"]),
        c if c != "gui" && crate::start::choice(c).is_some() => (Some(0), &[]),
        "compile" => (None, &["--sessions", "--dirty"]),
        "sets" => (Some(2), &["--docs", "--source", "--roots", "--name", "--json"]),
        "check" | "watch" | "decompile" => (None, &[]),
        "status" => (Some(0), &["--json", "--costs"]),
        "preview" | "explain" => (Some(1), &[]),
        "ripple" => (Some(1), &["--back"]),
        "context" => (Some(1), &["--depth", "--expand"]),
        "query" => (None, &["--json"]),
        "gen" => (None, &["--force"]),
        "test" => (None, &["--kind", "--list", "--audit", "--force"]),
        "codegen" | "testgen" => (None, &[]),
        "docsgen" | "agent" => (Some(0), &[]),
        // LSP clients append `--stdio` to name the transport (docs/frontends/cli.md#jazyk-lsp).
        "lsp" => (Some(0), &["--stdio"]),
        "viewer" => (Some(0), &[]),
        "gui" => (
            Some(0),
            &["--port", "--no-open", "--watch", "--gui-url", "--no-token", "--dev"],
        ),
        "mcp" => (
            Some(1),
            &[
                "--write",
                "--ephemeral",
                "--only",
                "--build-token",
                "--cancel-file",
                "--packaged",
                "--serve-files",
                "--edit-sink",
            ],
        ),
        "monitor" => (Some(0), &["--json", "--once"]),
        "release" => (Some(1), &[]),
        "answer" => (Some(1), &["--option", "--text"]),
        "spend" => (Some(2), &["--cached"]),
        "acp" => (Some(2), &["--ide"]),
        "benchmark" => (None, &["--project", "--goal", "--force"]),
        "replay" => (None, &[]),
        _ => return None,
    })
}

const COMMON_LLM: &str = "common: --llm-base-url URL  --model M  --api-key K  --out DIR";
const COMMON_OUT: &str = "common: --out DIR   the out directory (default <root>/.jazyk/)";

pub(crate) fn cmd_usage(cmd: &str) -> Option<String> {
    let s = match cmd {
        "sets" => "usage: jazyk sets list|add|update|remove|preview [NAME] [--docs GLOB] [--source GLOB] [--roots GLOB] [--name NAME]\n\nSelections are repeatable. Paths are relative to the project folder. Output is JSON (--json).".to_string(),
        c if c != "gui" && crate::start::choice(c).is_some() => {
            let ch = crate::start::choice(c).unwrap();
            format!(
                "usage: jazyk {} [--agent NAME]\n\n\
                 Set up {} for this project, then start it: the launcher choice of that\n\
                 name (bare `jazyk` lists them all). Setup is idempotent and corrects a\n\
                 stale entry in place. Outside a project the working directory is\n\
                 initialized first.\n\n\
                 options:\n  \
                 --agent NAME   for an IDE: the coding agent the Jazyk agent drives, recorded in [acp] agent\n\n\
                 exit: the started program's code; 1 when it is not installed or a setup step failed\n",
                c, ch.name
            )
        }
        "init" => "usage: jazyk init [--mcp claude|codex|gemini|opencode|pi|droid|cursor|none] [--agent NAME] [--acp IDE]\n\n\
             Initialize the current directory as a project root: write a minimal\n\
             jazyk.toml, scaffold docs/ (with a placeholder README.md) and\n\
             deliverable/, and offer MCP integration, the agent choice, and ACP\n\
             registration. Existing files are merged or left unchanged with a\n\
             warning, never overwritten. Each flag skips its prompt; a\n\
             non-interactive stdin skips or defaults the rest.\n\n\
             options:\n  \
             --mcp AGENT    the coding agent whose MCP config gains a jazyk entry (or none)\n  \
             --agent NAME   the ACP agent that does the AI work (embedded, claude, codex, opencode, none)\n  \
             --acp IDE      the ACP client to register jazyk with (or none)\n\n\
             exit: 0 when something was set up, 1 when nothing was written"
            .to_string(),
        "compile" => format!(
            "usage: jazyk compile [path...] [--dirty] [--set ID] [--sessions N]\n\n\
             Run one build. The reconciler derives the goal board from the documents\n\
             and the graph; sessions resolve ready goals batch by batch until the\n\
             board converges. Prints the board summary first, a `gc burst:` line as\n\
             each burst starts, goal lines as goals resolve or fail, and the verdict\n\
             with its counts last. Explicit paths skip project discovery and run ad\n\
             hoc on those files.\n\n\
             options:\n  \
             --sessions N    run at most N sessions, then stop with an honest incomplete\n  \
             \x20               (0 derives the board and parks every goal without an LLM call)\n  \
             --verbose, -v   every session's prompt, the goal cascade, raw payloads\n  \
             --quiet, -q     only the board summary, gc bursts, and the verdict\n\
             {}\n\n\
             exit: 0 on converged, 1 on incomplete, 2 on a usage error",
            COMMON_LLM
        ),
        "check" => format!(
            "usage: jazyk check [path...]\n\n\
             Compile, then exit non-zero if the build ends incomplete or open\n\
             diagnostics of severity error remain. The CI gate.\n\n\
             options:\n  \
             --verbose, -v   every session's prompt, the goal cascade, raw payloads\n  \
             --quiet, -q     only the board summary, gc bursts, and the verdict\n\
             {}\n\n\
             exit: 0 converged and clean, 1 otherwise",
            COMMON_LLM
        ),
        "watch" => format!(
            "usage: jazyk watch [path...]\n\n\
             Recompile on file change. Event bursts debounce, and a fingerprint over the\n\
             matched documents decides whether a build runs. An incomplete build retries\n\
             on its own with backoff until a file change resets it. Per build: the board\n\
             summary, one line per goal (opened with its cause, taken by a session,\n\
             resolved with its justification, failed, parked), and the verdict.\n\n\
             options:\n  \
             --verbose, -v   the full compile trace instead of goal lines\n  \
             --quiet, -q     only the board summary, gc bursts, and the verdict\n\
             {}",
            COMMON_LLM
        ),
        "status" => format!(
            "usage: jazyk status [--set ID] [--json] [--costs]\n\n\
             Without --set: one line per document set (state, generation, verdict, board\n\
             counts); --json prints the sets summary instead. With --set:\n\
             summarize status.yaml and the board: the store version and generation, the\n\
             last verdict with its counts, the live board counts (derived from disk the\n\
             way compile derives them), coverage, open diagnostics by severity, the\n\
             medium warning, the shape line (entities per depth, the fan-out histogram),\n\
             the last build's cost, the unattached remainder, the unclaimed report, and\n\
             `next:` lines naming the command that moves the board. Reads only.\n\n\
             options:\n  \
             --json    one JSON object with the same fields instead of text\n  \
             --costs   the journal's cost report instead: tokens by goal kind, by\n            \
             document, per requirement and per criterion, across every build,\n            \
             unmeasured sessions counted apart\n\
             {}",
            COMMON_OUT
        ),
        "spend" => format!(
            "usage: jazyk spend <generation> <tokens> [--cached N]\n\n\
             Record what an external agent's session billed on its journal entry, after\n\
             the fact: a session an external agent drove over MCP carries no tokens\n\
             unless the agent reported them (report_spend during the batch, or this\n\
             command on the committed entry). The entry takes tokens, cachedTokens when\n\
             given, and tokensSource: agent, so `jazyk status --costs` counts it as\n\
             agent-reported. Refuses an entry that already carries a figure, a\n\
             generation that is not a session entry, and one the journal does not hold.\n\
             Makes no LLM call; writes the one journal file.\n\n\
             arguments:\n  \
             <generation>   the session entry, g42 or 42\n  \
             <tokens>       the tokens the agent's endpoint billed, input and output\n\n\
             options:\n  \
             --cached N   the input tokens among them served from the prompt cache\n\
             {}\n\n\
             exit: 0; 1 when the entry is missing, is not a session, or already carries a figure; 2 on a bad argument",
            COMMON_OUT
        ),
        "preview" => format!(
            "usage: jazyk preview [goal|target|batch]\n\n\
             Render the next session's prompt exactly as the model would receive it:\n\
             the agent contract, the active skills, the project block, the goals\n\
             block, the loaded set with its handles, and the protocol line naming the\n\
             batch the session claims. With a goal id, the batch that goal would join;\n\
             with a target (a node id, a section reference, a document path), the batch\n\
             of the first ready goal on it; with a batch id (b<generation>-<n>), that\n\
             batch; without an argument, the batch the scheduler would claim next. A\n\
             goal that is not ready renders behind a `not ready:` line. `ratify` and\n\
             `answer` goals have no session; preview prints what the human owes\n\
             instead. Makes no LLM call and writes nothing.\n\n\
             {}\n\n\
             exit: 0 when a prompt was rendered, 1 when nothing rendered (the reason prints)",
            COMMON_OUT
        ),
        "explain" => format!(
            "usage: jazyk explain [goal|target]\n\n\
             For a goal: the change record that produced it, its cause, its class and\n\
             whether it is mandatory, its readiness, what blocks it, and its hints.\n\
             For a target (a node id, a section reference, a document path): the cone\n\
             of goals a change to it would open, plus the derived data a commit would\n\
             recompute. Without an argument: the whole board, one line per goal.\n\
             Makes no LLM call and writes nothing.\n\n\
             {}\n\n\
             exit: 0; 1 when the goal or target is unknown",
            COMMON_OUT
        ),
        "ripple" => format!(
            "usage: jazyk ripple [target|generation|doc] [--back]\n\n\
             Print the ripple DAG rooted at a change: every generation the root led\n\
             to, the goals each generation opened and the sessions that resolved\n\
             them, with causes and one-line justifications. A generation (g412, or\n\
             412) roots the whole-build report with its cost totals; a document path\n\
             roots at the last edit entry that dirtied it; a node id roots at the\n\
             last cascade that touched it. Without an argument, the last build.\n\
             Parked and failed goals print after the tree.\n\n\
             options:\n  \
             --back   walk causes instead of consequences, back to the human edit\n\
             {}\n\n\
             exit: 0; 1 when the root is unknown",
            COMMON_OUT
        ),
        "monitor" => format!(
            "usage: jazyk monitor [--json] [--once]\n\n\
             Watch the docs, the ledger, and the control plane; perform nothing. On\n\
             every state change print the ready goals on the board and which MCP tool\n\
             claims them (`goals`, then `begin_goals`), with blocked goals and their\n\
             reasons, then go quiet until the next change. Gated goals print as\n\
             awaiting release.\n\n\
             options:\n  \
             --json   one JSON object per notice instead of text\n  \
             --once   block until a goal is claimable, print that notice, exit 0\n\
             {}",
            COMMON_OUT
        ),
        "answer" => format!(
            "usage: jazyk answer <diagnostic|goal> [--option N | --text \"...\"]\n\n\
             Answer one prompted diagnostic from the terminal: a diagnostic id, a\n\
             ratify goal (resolved to its proposal), or an answer goal. Without a\n\
             reply, print the question and its options numbered from 0 and exit 1.\n\
             An edit option (a ratification proposal's accept) applies as a dual\n\
             write and resolves the diagnostic; the retract option is deterministic\n\
             too; any other reply records handling and runs its answer session here.\n\n\
             options:\n  \
             --option N     choose option N (numbered from 0 in the listing)\n  \
             --text \"...\"   a freeform reply; one of --option and --text, never both\n\
             {}\n\n\
             exit: 0 when the answer landed, 1 when nothing was written (the reason prints),\n\
             2 on a usage error",
            COMMON_OUT
        ),
        "release" => format!(
            "usage: jazyk release [compile|generate]\n\n\
             Record a release: approve the gated goals for the named stage (both when\n\
             unnamed) without running anything. The watchers wake, whichever worker\n\
             is attached does the work. The generate stage covers binding too;\n\
             decompilation releases through `jazyk decompile`. Prints how many gated\n\
             goals the release approved and what is ready now.\n\n\
             {}\n\n\
             exit: 0; 2 on an unknown stage",
            COMMON_OUT
        ),
        "context" => format!(
            "usage: jazyk context <ent:…|req:…|view:…|doc.md#/ref|h:…> [--depth N] [--expand HANDLE]...\n\n\
             Print what the `load` tool renders for a target: the target in full, its\n\
             edges, each neighbor as a stub, and the status block of the loaded set\n\
             with its handles. Exactly what a session sees, under the same context\n\
             budget.\n\n\
             options:\n  \
             --depth N        neighbor depth (default 1)\n  \
             --expand HANDLE  follow the named handle before printing (repeatable)\n\
             {}",
            COMMON_OUT
        ),
        "query" => format!(
            "usage: jazyk query <text> [--json]\n\n\
             Search entities. Prints one `id (name): definition` line per match.\n\n\
             options:\n  \
             --json   one {{\"id\", \"name\", \"definition\"}} object per line instead\n\
             {}\n\n\
             exit: 0 when something matched, 1 when nothing did",
            COMMON_OUT
        ),
        "gen" => format!(
            "usage: jazyk gen [entity...]\n\n\
             Run the built-in worker to develop source and tests together,\n\
             choosing their order within one bounded session per\n\
             entity, and record the manifest in the ledger. With no arguments, cover\n\
             every entity that has at least one requirement, leaf entities first,\n\
             skipping entities whose facts are unchanged.\n\n\
             options:\n  \
             --force   regenerate even when facts are unchanged\n\
             {}",
            COMMON_LLM
        ),
        "test" => format!(
            "usage: jazyk test [target...]\n\n\
             Run verification over the ledger: the verify goals. Entity ids select\n\
             their requirements' rows; requirement ids select rows directly.\n\n\
             options:\n  \
             --kind programmatic|llm   only rows of this kind\n  \
             --list                    print the status table (id, statement, status) without running\n  \
             --audit                   rebuild the ledger from the artifact markers\n  \
             --force                   also rerun verified rows\n\
             {}\n\n\
             exit: 0 when every targeted row is verified, 1 otherwise",
            COMMON_LLM
        ),
        "decompile" => format!(
            "usage: jazyk decompile [path...]\n\n\
             Draft documents describing what the code under the named scopes does\n\
             (default: every scope in the unclaimed report). Records the decompile\n\
             release; with an agent attached and preferred, the agent's watcher does\n\
             the drafting, otherwise the built-in worker runs. Drafts land in the docs\n\
             tree carrying an unratified diagnostic until edited; the next compile\n\
             extracts them and binding self-checks the statements against the code.\n\n\
             {}",
            COMMON_LLM
        ),
        "docsgen" => format!(
            "usage: jazyk docsgen\n\n\
             Render the documentation pages into <out>/docsgen/ without compiling:\n\
             the per-entity requirements documents, the entity cards (entities/), the\n\
             level pages (levels/), and the diagram pages (diagrams/<kind>/). The\n\
             summary counts every page written. The diagrams they embed render with\n\
             them, skipped for unchanged .puml content, so every image link resolves.\n\n\
             {}",
            COMMON_OUT
        ),
        "viewer" => "usage: jazyk viewer [--out FILE]\n\n\
             Render the graph to one self-contained HTML page. An --out ending in .html\n\
             names the file (default <out>/graph.html); otherwise --out is the out\n\
             directory."
            .to_string(),
        "gui" => format!(
            "usage: jazyk gui [--port N] [--no-open] [--watch] [--gui-url URL] [--no-token] [--dev]\n\n\
             Start the GUI's server: the JSON API, the event stream, and the language\n\
             server over WebSocket, then open the editor page at jazyk.dev with the port\n\
             and the token in the address, so the page connects to this server. The\n\
             binary carries no page. Binds 127.0.0.1 only. The busy default port falls\n\
             back to an ephemeral one; an explicit --port that is busy is an error.\n\n\
             options:\n  \
             --port N        the port (default 4680)\n  \
             --no-open       print the address instead of opening the browser\n  \
             --watch         start in watch mode: compile on change (default: queue)\n  \
             --gui-url URL   open another copy of the page (a dev server, a fork) and\n                  \
             admit that origin (default https://jazyk.dev, or JAZYK_GUI_URL)\n  \
             --no-token      disable the session token check (frontend development)\n  \
             --dev           start with the Developer toggle on (the compiler workbench)\n  \
\
             {}",
            COMMON_LLM
        ),
        "mcp" => format!(
            "usage: jazyk mcp <toolsets>\n\n\
             Serve the tool registry over stdio as an MCP server. <toolsets> is a comma\n\
             list of: compile (claim goal batches: goals, begin_goals, done), generate\n\
             (the binding and generation workflows), verify (the verification\n\
             workflow), decompile (draft docs for unclaimed code), benchmark (the\n\
             agent under test performs the cases against sandbox stores), chat (the\n\
             chat serving: reads, lifecycles, dual-write tools, no raw writes),\n\
             graph (read tools; --write adds raw write tools), all (compile,\n\
             generate, verify, decompile, chat in one serving; what init writes).\n\n\
             Bridge flags (set by the ACP bridge when it injects a serving into a\n\
             session; not for standalone servings):\n  \
             --ephemeral          the serving belongs to one session\n  \
             --only ID            begin_goals accepts only this batch (or a goal, selecting its batch)\n  \
             --build-token ID     part of the running internal build\n  \
             --cancel-file PATH   stop fence for this session, disables implicit finish\n  \
             --packaged           the prompt was already delivered; begin_goals acks instead of repeating it\n  \
             --serve-files        add the sandboxed file and command tools\n  \
             --edit-sink PATH     delegate document and settings writes to the spawning process\n\n\
             {}",
            COMMON_OUT
        ),
        "agent" => "usage: jazyk agent\n\n\
             The embedded ACP agent over stdio: a generic agent over the configured\n\
             LLM endpoint with no jazyk knowledge. The bridge spawns it when the\n\
             `embedded` profile is selected. Not meant to be run by hand."
            .to_string(),
        "acp" => "usage: jazyk acp [install --ide <client>]\n\n\
             Without arguments: the IDE-facing ACP proxy on stdio. An IDE spawns it\n\
             as its Jazyk agent; it drives the configured downstream agent and adds\n\
             jazyk in between: tool injection, doc edit delegation, slash commands,\n\
             build status. Outside a jazyk project it is a transparent passthrough.\n\
             Not meant to be run by hand.\n\n\
             jazyk acp install --ide <client> registers Jazyk with an ACP client:\n\
             zed, jetbrains, vscode, neovim, emacs, obsidian, acpx, marimo. The\n\
             client may also be given positionally (jazyk acp install zed). A config\n\
             jazyk writes is merged in place, leaving comments and other agents\n\
             alone; for the rest, the snippet to paste is printed.\n\n\
             exit: 0 when the entry was written, was already current, or the snippet\n\
             was printed, 1 when a file could not be written"
            .to_string(),
        "lsp" => format!(
            "usage: jazyk lsp [--stdio]\n\n\
             Language server over stdio. Read-only: serves the last committed graph, and\n\
             a compile or watch rebuild refreshes it. --stdio is accepted and changes\n\
             nothing: LSP clients append it to name the transport.\n\n\
             {}",
            COMMON_OUT
        ),
        "replay" => "usage: jazyk replay [file|dir...]\n       jazyk replay extract <trace.jsonl> <label>\n\n\
             Run replays: cases whose tool calls are scripted, graded against the\n\
             harness without a model. No argument runs docs/benchmark/replays.\n\
             `extract` writes one session of a trace as a replay to stdout.\n\n\
             exit: 0 when every step and check holds, 1 otherwise".to_string(),
        "benchmark" => format!(
            "usage: jazyk benchmark [case...]\n\n\
             Grade the configured model: benchmark cases use Goose GDK with native\n\
             tool calls in a sandbox store, scored by\n\
             deterministic checks. Results land in <out>/benchmark/results.yaml.\n\n\
             options:\n  \
             --project <dir> --goal <id>   run one goal's session from a copy of a real project\n  \
             --force                       run the goal even when the board holds it blocked\n\
             {}",
            COMMON_LLM
        ),
        _ => return None,
    };
    Some(s)
}

pub fn run() {
    load_dotenv();
    let args: Vec<String> = std::env::args().collect();
    #[cfg(feature = "native-documents")]
    if args.get(1).is_some_and(|s| s == "document-read") {
        let result = args.get(2).ok_or_else(|| "usage: jazyk document-read FILE".to_string())
            .and_then(|p| crate::documents::read(std::path::Path::new(p)));
        match result {
            Ok(p) => println!("{}", serde_json::to_string_pretty(&p).unwrap()),
            Err(e) => { eprintln!("jazyk: {e}"); std::process::exit(1); }
        }
        return;
    }
    let mut opts = cli::Options::default();
    let mut positional: Vec<String> = Vec::new();
    let mut cmd = String::new();
    let mut want_help = false;
    // Every option seen, checked against the command's shape once the command is
    // known (it may follow its options: `jazyk --out x status`).
    let mut seen: Vec<String> = Vec::new();
    // Usage errors wait for the command name so the message can point at its help.
    let mut errors: Vec<String> = Vec::new();
    // A whole number, or a usage error naming the option and what it got.
    fn number<T: std::str::FromStr>(flag: &str, v: &str, errors: &mut Vec<String>) -> Option<T> {
        match v.parse::<T>() {
            Ok(n) => Some(n),
            Err(_) => {
                errors.push(format!("{} takes a whole number, got `{}`", flag, v));
                None
            }
        }
    }
    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        if VALUE_OPTIONS.contains(&arg) {
            seen.push(arg.to_string());
            i += 1;
            let Some(v) = args.get(i).cloned() else {
                errors.push(format!("{} takes a value", arg));
                break;
            };
            match arg {
                "--llm-base-url" => opts.base_url = Some(v),
                "--model" => opts.model = Some(v),
                "--api-key" => opts.api_key = Some(v),
                "--out" => opts.out = Some(v),
                "--set" => opts.set = Some(v),
                "--docs" => opts.docs.push(v),
                "--source" => opts.source.push(v),
                "--roots" => opts.roots.push(v),
                "--name" => opts.name = Some(v),
                "--depth" => opts.depth = number(arg, &v, &mut errors),
                "--expand" => opts.expand.push(v),
                "--kind" => opts.kind = Some(v),
                "--port" => opts.port = number(arg, &v, &mut errors),
                "--gui-url" => opts.gui_url = Some(v),
                "--mcp" => opts.mcp = Some(v),
                "--agent" => opts.agent = Some(v),
                // `--acp` names the editor during init; `--ide` is the same argument
                // spelled for `acp install`. Mirrors docs/frontends/cli.md#jazyk-acp.
                "--acp" | "--ide" => opts.acp_ide = Some(v),
                "--only" => opts.only = Some(v),
                "--project" => opts.project = Some(v),
                "--goal" => opts.goal = Some(v),
                "--sessions" => opts.sessions = number(arg, &v, &mut errors),
                "--option" => opts.option = number(arg, &v, &mut errors),
                "--text" => opts.text = Some(v),
                "--build-token" => opts.build_token = Some(v),
                "--cancel-file" => opts.cancel_file = Some(v),
                "--edit-sink" => opts.edit_sink = Some(v),
                "--cached" => opts.cached = number(arg, &v, &mut errors),
                _ => unreachable!("every value option is matched"),
            }
            i += 1;
            continue;
        }
        match arg {
            "--help" | "-h" => want_help = true,
            "help" if cmd.is_empty() => want_help = true,
            "--back" => opts.back = true,
            "--dirty" => opts.dirty = true,
            "--ephemeral" => opts.ephemeral = true,
            "--packaged" => opts.packaged = true,
            "--serve-files" => opts.serve_files = true,
            "--no-open" => opts.no_open = true,
            "--watch" => opts.watch = true,
            "--no-token" => opts.no_token = true,
            "--dev" => opts.dev = true,
            "--verbose" | "-v" => opts.verbose = true,
            "--quiet" | "-q" => opts.quiet = true,
            "--write" => opts.write = true,
            "--json" => opts.json = true,
            "--once" => opts.once = true,
            "--costs" => opts.costs = true,
            "--force" => opts.force = true,
            "--list" => opts.list = true,
            "--audit" => opts.audit = true,
            // LSP clients append it to name the transport; `lsp` honors it and it
            // changes nothing (docs/frontends/cli.md#jazyk-lsp).
            "--stdio" => {}
            // An option nobody defined is a typo, never a positional: `status --bogus`
            // must not become an explicit document path.
            s if s.starts_with('-') && s.len() > 1 => {
                errors.push(format!("unknown option `{}`", s));
            }
            s if cmd.is_empty() => cmd = s.to_string(),
            s => positional.push(s.to_string()),
        }
        if arg.starts_with('-') && arg.len() > 1 {
            seen.push(arg.to_string());
        }
        i += 1;
    }

    // The --agent flag rides the environment so every resolver on the ladder sees it
    // (docs/compiler/project-settings.md#acp).
    if let Some(a) = &opts.agent {
        std::env::set_var("JAZYK_ACP_AGENT", a);
    }

    // Help prints to stdout and exits 0; help for a command that does not exist is
    // the unknown-command error. Mirrors docs/frontends/cli.md#help.
    if want_help {
        let key = match cmd.as_str() {
            "codegen" | "testgen" => "gen",
            c => c,
        };
        if key.is_empty() {
            println!("{}", top_usage());
            std::process::exit(0);
        }
        match cmd_usage(key) {
            Some(u) => {
                println!("{}", u);
                std::process::exit(0);
            }
            None => usage_error("", &format!("unknown command `{}`", cmd)),
        }
    }
    // Bare `jazyk` is the launcher (docs/frontends/cli.md#jazyk): init outside a
    // project, the status and a way to start inside one.
    if cmd.is_empty() {
        if let Some(e) = errors.first() {
            usage_error("", e);
        }
        std::process::exit(crate::start::run(&opts, None));
    }
    let Some((arity, extra)) = command_shape(&cmd) else {
        usage_error("", &format!("unknown command `{}`", cmd));
    };
    if let Some(e) = errors.first() {
        usage_error(&cmd, e);
    }
    // The options a command honors are exactly the ones its help lists; anything
    // else is refused rather than ignored (`check --sessions 0` would otherwise run
    // every session). Mirrors docs/frontends/cli.md#help.
    if let Some(stray) = seen
        .iter()
        .find(|o| !COMMON_OPTIONS.contains(&o.as_str()) && !extra.contains(&o.as_str()))
    {
        usage_error(&cmd, &format!("`{}` does not take {}", cmd, stray));
    }
    if let Some(max) = arity {
        if positional.len() > max {
            let got = positional
                .iter()
                .map(|p| format!("`{}`", p))
                .collect::<Vec<_>>()
                .join(", ");
            let takes = match max {
                0 => "no arguments".to_string(),
                1 => "one argument".to_string(),
                n => format!("at most {} arguments", n),
            };
            usage_error(&cmd, &format!("`{}` takes {} (got {})", cmd, takes, got));
        }
    }

    if !["watch","query","init","sets","gui","lsp","acp","agent","mcp","benchmark","compile","status","replay"].contains(&cmd.as_str()) && crate::start::choice(&cmd).is_none() {
        let (proj,_,_)=cli::resolve(&[],&opts);
        if let Some(error)=proj.config_error { usage_error(&cmd,&error); }
        if proj.set_id.is_none() { usage_error(&cmd,"choose a document set with --set NAME"); }
    }
    let code = match cmd.as_str() {
        "sets" => cli::run_sets(&positional, &opts),
        "init" => cli::run_init(&opts),
        c if c != "gui" && crate::start::choice(c).is_some() => crate::start::run(&opts, Some(c)),
        "compile" => cli::run_compile(&positional, &opts),
        "check" => cli::run_check(&positional, &opts),
        "watch" => cli::run_watch(&positional, &opts),
        "status" => cli::run_status(&opts),
        "preview" => cli::run_preview(&positional, &opts),
        "explain" => cli::run_explain(&positional, &opts),
        "ripple" => cli::run_ripple(&positional, &opts),
        "spend" => cli::run_spend(&positional, &opts),
        "context" => match positional.first() {
            Some(target) => cli::run_context(&opts, target),
            None => usage_error(
                "context",
                "`context` takes a target: a node id (ent:..., req:..., view:...) or a section reference (doc.md#/ref)",
            ),
        },
        "query" => {
            if positional.is_empty() {
                usage_error("query", "`query` takes the text to search for");
            }
            let q = positional.join(" ");
            cli::run_query(&opts, &q)
        }
        "mcp" => match positional.first().map(|s| s.as_str()) {
            Some(arg) => {
                // The toolsets served, comma separated: compile, generate, verify,
                // decompile, benchmark, chat, graph.
                // `all` is the union an agent working a whole project needs
                // (docs/frontends/mcp.md#toolsets): benchmark and graph stay out.
                let mut modes: Vec<String> = Vec::new();
                for m in arg.split(',').map(|m| m.trim()).filter(|m| !m.is_empty()) {
                    let expanded: &[&str] = if m == "all" {
                        &["compile", "generate", "verify", "decompile", "chat"]
                    } else {
                        std::slice::from_ref(&m)
                    };
                    for e in expanded {
                        if !modes.iter().any(|x| x == e) {
                            modes.push((*e).to_string());
                        }
                    }
                }
                let known = [
                    "runtime",
                    "compile",
                    "generate",
                    "verify",
                    "decompile",
                    "benchmark",
                    "graph",
                    "chat",
                ];
                if let Some(bad) = modes.iter().find(|m| !known.contains(&m.as_str())) {
                    usage_error(
                        "mcp",
                        &format!("unknown toolset `{}`; one of {}", bad, known.join(", ")),
                    );
                }
                if modes.is_empty() {
                    usage_error("mcp", "`mcp` takes the toolsets to serve, comma separated");
                }
                if opts.write && !modes.iter().any(|m| m == "graph") {
                    usage_error(
                        "mcp",
                        "--write applies to the graph toolset only; compile gates writes behind begin_goals",
                    );
                } else {
                    let (mut proj, _llm, mut out) = cli::resolve(&[], &opts);
                    if opts.set.is_none() && opts.only.is_none() && opts.build_token.is_none() {
                        if let Ok(w)=crate::document_sets::Workspace::load(&proj.root) {proj=w.files_project();out=proj.root.join(".jazyk");}
                    }
                    let bridge = mcp::BridgeFlags {
                        ephemeral: opts.ephemeral,
                        only: opts.only.clone(),
                        build_token: opts.build_token.clone(),
                        cancel_file: opts.cancel_file.as_ref().map(std::path::PathBuf::from),
                        serve_files: opts.serve_files,
                        edit_sink: opts.edit_sink.clone(),
                        packaged: opts.packaged,
                        reader: false,
                    };
                    mcp::McpServer::with_bridge(proj, out, modes, opts.write, bridge).run();
                    0
                }
            }
            _ => usage_error("mcp", "`mcp` takes the toolsets to serve, comma separated"),
        },
        "monitor" => cli::run_monitor(&opts),
        "release" => cli::run_release(&positional, &opts),
        "answer" => cli::run_answer(&positional, &opts),
        "gen" => cli::run_gen(&opts, &positional),
        "test" => cli::run_test(&opts, &positional),
        "decompile" => cli::run_decompile(&opts, &positional),
        // A pointer only, never the work: docs/frontends/cli.md#jazyk-gen.
        "codegen" | "testgen" => {
            eprintln!("jazyk: `{}` is deprecated; generation is one workflow now, use `jazyk gen` (and `jazyk test` to verify)", cmd);
            2
        }
        "docsgen" => {
            let (proj, _llm, out) = cli::resolve(&[], &opts);
            let store = store::Store::load(&out);
            // The diagrams the pages embed render first, skipped for unchanged .puml
            // content, so every image link resolves. Mirrors
            // docs/frontends/cli.md#jazyk-docsgen.
            let dr = render::render_all(&store, &out);
            let n = docsgen::write_all(&store, &gen::GenSettings::resolve(&proj));
            let mut line = format!(
                "jazyk: docsgen: {} page(s) in {}; diagrams: {} drawn, {} unchanged",
                n,
                out.join("docsgen").display(),
                dr.rendered.len(),
                dr.skipped.len()
            );
            if !dr.failed.is_empty() {
                line.push_str(&format!(", {} failed", dr.failed.len()));
            }
            println!("{}", line);
            0
        }
        "viewer" => cli::run_viewer(&opts),
        "gui" => cli::run_gui(&positional, &opts),
        "lsp" => {
            let (proj, _llm, out) = cli::resolve(&positional, &opts);
            let gs = gen::GenSettings::resolve(&proj);
            lsp::Lsp::new(proj.root.clone(), if opts.set.is_some() {out} else {proj.root.join(".jazyk")}, gs).run();
            0
        }
        // Replays: scripted sessions against the harness, no model
        // (docs/benchmark/replays.md).
        "replay" => replay::run_cli(&positional),
        "benchmark" => {
            let (_proj, llm, out) = cli::resolve(&[], &opts);
            // `--goal` runs one goal's session from a copy of a real project.
            // Mirrors docs/benchmark/benchmark.md#snippets-from-a-real-project.
            if let Some(goal) = opts.goal.as_deref() {
                let root = opts
                    .project
                    .as_deref()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| {
                        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                    });
                benchmark::run_goal(&llm, &root, goal, opts.force)
            } else {
                benchmark::run_filtered(&llm, &out, &positional)
            }
        }
        // The embedded ACP agent, spawned by the host when the `embedded` profile is
        // selected. Not meant to be run by hand. Mirrors docs/frontends/cli.md#jazyk-agent.
        "agent" => acp::agent::run(),
        // The IDE-facing proxy, and its registry installer.
        // Mirrors docs/frontends/cli.md#jazyk-acp.
        "acp" => match positional.first().map(|s| s.as_str()) {
            Some("install") => cli::run_acp_install(
                positional
                    .get(1)
                    .map(|s| s.as_str())
                    .or(opts.acp_ide.as_deref()),
            ),
            _ => acp::proxy::run(&opts),
        },
        _ => usage(),
    };
    std::process::exit(code);
}
