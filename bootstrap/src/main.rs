mod acp;
mod align;
mod answer;
mod benchmark;
mod bind;
mod board;
mod cli;
mod context;
mod control;
mod decompile;
mod derive;
mod docsgen;
mod feedback;
mod gen;
mod goals;
mod gui;
mod jsonrpc;
mod limits;
mod llm;
mod lsp;
mod mcp;
mod md;
mod model;
mod parallel;
mod project;
mod reconcile;
mod render;
mod session;
mod store;
mod tools;
mod verify;
mod viewer;

// Load a .env file by walking up from the current directory. Does not override existing env vars.
fn load_dotenv() {
    let mut dir = std::env::current_dir().ok();
    while let Some(d) = dir {
        let f = d.join(".env");
        if f.exists() {
            if let Ok(content) = std::fs::read_to_string(&f) {
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

fn top_usage() -> String {
    let mut s = String::new();
    s.push_str("jazyk: natural language compiler (goal-based)\n\n");
    s.push_str("usage:\n");
    s.push_str("  jazyk init                     scaffold a project (jazyk.toml, docs/, deliverable/) here\n");
    s.push_str("  jazyk compile [path...]        run one build: sessions resolve goals until the board converges\n");
    s.push_str(
        "  jazyk check [path...]          compile, exit non-zero on error diagnostics (CI)\n",
    );
    s.push_str("  jazyk watch [path...]          recompile on change, one line per goal\n");
    s.push_str(
        "  jazyk status                   the store version, the last verdict, the board counts\n",
    );
    s.push_str("  jazyk preview [goal|target]    the next session's prompt, exactly as the model receives it\n");
    s.push_str(
        "  jazyk explain [goal|target]    why a goal exists, or what a change to a target opens\n",
    );
    s.push_str("  jazyk ripple [root] [--back]   walk a change's cascade through the journal (id, g412, or doc)\n");
    s.push_str("  jazyk context <target>         what the `load` tool renders (--depth N, --expand HANDLE)\n");
    s.push_str("  jazyk query <text>             search entities\n");
    s.push_str("  jazyk gen [entity...]          resolve the bind and generate goals into the deliverable (--force)\n");
    s.push_str("  jazyk test [target...]         resolve the verify goals (--kind programmatic|llm, --list, --audit, --force)\n");
    s.push_str("  jazyk decompile [path...]      draft docs describing what unclaimed code does\n");
    s.push_str(
        "  jazyk docsgen                  render per-entity requirements documents and their diagrams\n",
    );
    s.push_str("  jazyk viewer [--out FILE]      render the graph to a self-contained HTML page\n");
    s.push_str(
        "  jazyk gui [--port N]           local GUI: web app, API, events, LSP over WebSocket\n",
    );
    s.push_str("  jazyk mcp <toolsets>           the MCP server: compile,generate,verify,decompile,benchmark,chat,graph\n  jazyk monitor [--json] [--once]  print ready and blocked goals as the state changes; --once exits at the first\n  jazyk release [compile|generate]  approve gated goals in manual mode without running anything\n");
    s.push_str("  jazyk lsp                      language server over stdio (read-only; compile or watch rebuilds)\n");
    s.push_str("  jazyk agent                    the embedded ACP agent over stdio (spawned by the bridge)\n");
    s.push_str("  jazyk benchmark [case...]      grade the configured agent and model\n");
    s.push_str("\noptions: --agent NAME  --llm-base-url URL  --model M  --api-key K  --out DIR\n");
    s.push_str(
        "         --verbose, -v   the full session trace: prompts, payloads, the goal cascade\n",
    );
    s.push_str("         --quiet, -q     only the board summary, gc bursts, and the verdict\n");
    s.push_str("         --help, -h      print help and exit\n");
    s
}

fn usage() -> ! {
    eprintln!("{}", top_usage());
    std::process::exit(2);
}

const COMMON_LLM: &str = "common: --llm-base-url URL  --model M  --api-key K  --out DIR";
const COMMON_OUT: &str = "common: --out DIR   the out directory (default <root>/jazyk-out/)";

fn cmd_usage(cmd: &str) -> Option<String> {
    let s = match cmd {
        "init" => "usage: jazyk init [--mcp claude|cursor|vscode|gemini|none] [--agent NAME] [--acp IDE]\n\n\
             Initialize the current directory as a project root: write a minimal\n\
             jazyk.toml, scaffold docs/ (with a placeholder README.md) and\n\
             deliverable/, and offer MCP integration, the agent choice, and ACP\n\
             registration. Existing files are merged or left unchanged with a\n\
             warning, never overwritten. Each flag skips its prompt; a\n\
             non-interactive stdin skips or defaults the rest.\n\n\
             options:\n  \
             --mcp AGENT    the coding agent whose MCP config gains a jazyk entry (or none)\n  \
             --agent NAME   the ACP agent that does the AI work (embedded, codex, claude, opencode, none)\n  \
             --acp IDE      the ACP client to register jazyk with (or none)\n\n\
             exit: 0 when something was set up, 1 when nothing was written"
            .to_string(),
        "compile" => format!(
            "usage: jazyk compile [path...]\n\n\
             Run one build. The reconciler derives the goal board from the documents\n\
             and the graph; sessions resolve ready goals batch by batch until the\n\
             board converges. Prints the board summary first, a `gc burst:` line as\n\
             each burst starts, goal lines as goals resolve or fail, and the verdict\n\
             with its counts last. Explicit paths skip project discovery and run ad\n\
             hoc on those files.\n\n\
             options:\n  \
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
            "usage: jazyk status\n\n\
             Summarize status.yaml and the board: the store version and generation, the\n\
             last verdict with its counts, the live board counts (derived from disk the\n\
             way compile derives them), coverage, open diagnostics by severity, the\n\
             last build's cost, and the unclaimed report.\n\n\
             {}",
            COMMON_OUT
        ),
        "preview" => format!(
            "usage: jazyk preview [goal|target]\n\n\
             Render the next session's prompt exactly as the model would receive it:\n\
             the agent contract, the active skills, the project block, the goals\n\
             block, the loaded set with its handles, and the protocol line. With a\n\
             goal id, the batch that goal would join; with a target, the batch of the\n\
             first goal on it; without an argument, the batch the scheduler would\n\
             claim next. A goal that is not ready renders behind a `not ready:` line.\n\
             `ratify` and `answer` goals have no session; preview prints what the\n\
             human owes instead. Makes no LLM call and writes nothing.\n\n\
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
        "release" => format!(
            "usage: jazyk release [compile|generate]\n\n\
             Record a release: approve the gated goals for the named stage (both when\n\
             unnamed) without running anything. The watchers wake, whichever worker\n\
             is attached does the work. The generate stage covers binding too;\n\
             decompilation releases through `jazyk decompile`.\n\n\
             {}",
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
            "usage: jazyk query <text>\n\n\
             Search entities. Prints one {{id, name, definition}} line per match.\n\n\
             {}",
            COMMON_OUT
        ),
        "gen" => format!(
            "usage: jazyk gen [entity...]\n\n\
             Run the built-in generation worker over the ledger goals: resolve owed\n\
             bind goals first, then the generate goals, one bounded session per\n\
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
             Render the per-entity requirements documents into <out>/docsgen/ without\n\
             compiling. The diagrams they embed re-render with them, skipped for\n\
             unchanged .puml content, so every image link resolves.\n\n\
             {}",
            COMMON_OUT
        ),
        "viewer" => "usage: jazyk viewer [--out FILE]\n\n\
             Render the graph to one self-contained HTML page. An --out ending in .html\n\
             names the file (default <out>/graph.html); otherwise --out is the out\n\
             directory."
            .to_string(),
        "gui" => format!(
            "usage: jazyk gui [--port N] [--no-open] [--watch] [--gui-dist DIR] [--no-token]\n\n\
             Start the GUI: one local server with the web app, the JSON API, the event\n\
             stream, and the language server over WebSocket, then open the browser.\n\
             Binds 127.0.0.1 only. The busy default port falls back to an ephemeral one;\n\
             an explicit --port that is busy is an error.\n\n\
             options:\n  \
             --port N        the port (default 4680)\n  \
             --no-open       do not open the browser\n  \
             --watch         start in watch mode: compile on change (default: queue)\n  \
             --gui-dist DIR  serve frontend assets from DIR instead of the embedded build\n  \
             --no-token      disable the session token check (frontend development)\n\
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
             graph (read tools; --write adds raw write tools).\n\n\
             Bridge flags (set by the ACP bridge when it injects a serving into a\n\
             session; not for standalone servings):\n  \
             --ephemeral          the serving belongs to one session\n  \
             --only ID            begin_goals accepts only this batch (or a goal, selecting its batch)\n  \
             --build-token ID     part of the running internal build\n  \
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
            "usage: jazyk lsp\n\n\
             Language server over stdio. Read-only: serves the last committed graph, and\n\
             a compile or watch rebuild refreshes it.\n\n\
             {}",
            COMMON_OUT
        ),
        "benchmark" => format!(
            "usage: jazyk benchmark [case...]\n\n\
             Grade the configured model: every benchmark case runs under both codecs\n\
             (native tool calls and the text codec) in a sandbox store, scored by\n\
             deterministic checks. Results land in <out>/benchmark/results.yaml.\n\n\
             {}",
            COMMON_LLM
        ),
        _ => return None,
    };
    Some(s)
}

fn main() {
    load_dotenv();
    let args: Vec<String> = std::env::args().collect();
    let mut opts = cli::Options::default();
    let mut positional: Vec<String> = Vec::new();
    let mut cmd = String::new();
    let mut want_help = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => want_help = true,
            "help" if cmd.is_empty() => want_help = true,
            "--llm-base-url" => {
                i += 1;
                opts.base_url = args.get(i).cloned();
            }
            "--model" => {
                i += 1;
                opts.model = args.get(i).cloned();
            }
            "--api-key" => {
                i += 1;
                opts.api_key = args.get(i).cloned();
            }
            "--out" => {
                i += 1;
                opts.out = args.get(i).cloned();
            }
            "--depth" => {
                i += 1;
                opts.depth = args.get(i).and_then(|s| s.parse::<u32>().ok());
            }
            "--expand" => {
                i += 1;
                if let Some(h) = args.get(i) {
                    opts.expand.push(h.clone());
                }
            }
            "--kind" => {
                i += 1;
                opts.kind = args.get(i).cloned();
            }
            "--port" => {
                i += 1;
                opts.port = args.get(i).and_then(|s| s.parse::<u16>().ok());
            }
            "--gui-dist" => {
                i += 1;
                opts.gui_dist = args.get(i).cloned();
            }
            "--mcp" => {
                i += 1;
                opts.mcp = args.get(i).cloned();
            }
            "--agent" => {
                i += 1;
                opts.agent = args.get(i).cloned();
            }
            // `--acp` names the editor during init; `--ide` is the same argument
            // spelled for `acp install`. Mirrors docs/frontends/cli.md#jazyk-acp.
            "--acp" | "--ide" => {
                i += 1;
                opts.acp_ide = args.get(i).cloned();
            }
            "--only" => {
                i += 1;
                opts.only = args.get(i).cloned();
            }
            "--project" => {
                i += 1;
                opts.project = args.get(i).cloned();
            }
            "--goal" => {
                i += 1;
                opts.goal = args.get(i).cloned();
            }
            "--sessions" => {
                i += 1;
                opts.sessions = args.get(i).and_then(|s| s.parse().ok());
            }
            "--build-token" => {
                i += 1;
                opts.build_token = args.get(i).cloned();
            }
            "--edit-sink" => {
                i += 1;
                opts.edit_sink = args.get(i).cloned();
            }
            "--back" => opts.back = true,
            "--ephemeral" => opts.ephemeral = true,
            "--packaged" => opts.packaged = true,
            "--serve-files" => opts.serve_files = true,
            "--no-open" => opts.no_open = true,
            "--watch" => opts.watch = true,
            "--no-token" => opts.no_token = true,
            "--verbose" | "-v" => opts.verbose = true,
            "--quiet" | "-q" => opts.quiet = true,
            "--write" => opts.write = true,
            "--json" => opts.json = true,
            "--once" => opts.once = true,
            "--force" => opts.force = true,
            "--list" => opts.list = true,
            "--audit" => opts.audit = true,
            s if cmd.is_empty() => cmd = s.to_string(),
            s => positional.push(s.to_string()),
        }
        i += 1;
    }

    // The --agent flag rides the environment so every resolver on the ladder sees it
    // (docs/compiler/project-settings.md#acp).
    if let Some(a) = &opts.agent {
        std::env::set_var("JAZYK_ACP_AGENT", a);
    }

    if want_help {
        let key = match cmd.as_str() {
            "codegen" | "testgen" => "gen",
            c => c,
        };
        match cmd_usage(key) {
            Some(u) => println!("{}", u),
            None => println!("{}", top_usage()),
        }
        std::process::exit(0);
    }
    if cmd.is_empty() {
        usage();
    }

    let code = match cmd.as_str() {
        "init" => cli::run_init(&opts),
        "compile" => cli::run_compile(&positional, &opts),
        "check" => cli::run_check(&positional, &opts),
        "watch" => cli::run_watch(&positional, &opts),
        "status" => cli::run_status(&positional, &opts),
        "preview" => cli::run_preview(&positional, &opts),
        "explain" => cli::run_explain(&positional, &opts),
        "ripple" => cli::run_ripple(&positional, &opts),
        "context" => match positional.first() {
            Some(target) => cli::run_context(&positional[1..], &opts, target),
            None => {
                eprintln!("{}", cmd_usage("context").unwrap());
                2
            }
        },
        "query" => {
            if positional.is_empty() {
                eprintln!("{}", cmd_usage("query").unwrap());
                2
            } else {
                let q = positional.join(" ");
                cli::run_query(&[], &opts, &q)
            }
        }
        "mcp" => match positional.first().map(|s| s.as_str()) {
            Some(arg) => {
                // The toolsets served, comma separated: compile, generate, verify,
                // decompile, benchmark, chat, graph.
                let modes: Vec<String> = arg
                    .split(',')
                    .map(|m| m.trim().to_string())
                    .filter(|m| !m.is_empty())
                    .collect();
                let known = [
                    "compile",
                    "generate",
                    "verify",
                    "decompile",
                    "benchmark",
                    "graph",
                    "chat",
                ];
                if modes.is_empty() || modes.iter().any(|m| !known.contains(&m.as_str())) {
                    eprintln!("{}", cmd_usage("mcp").unwrap());
                    2
                } else if opts.write && !modes.iter().any(|m| m == "graph") {
                    eprintln!("jazyk: --write applies to the graph toolset only; compile gates writes behind begin_goals");
                    2
                } else {
                    let (proj, _llm, out) = cli::resolve(&[], &opts);
                    let bridge = mcp::BridgeFlags {
                        ephemeral: opts.ephemeral,
                        only: opts.only.clone(),
                        build_token: opts.build_token.clone(),
                        serve_files: opts.serve_files,
                        edit_sink: opts.edit_sink.clone(),
                        packaged: opts.packaged,
                    };
                    mcp::McpServer::with_bridge(proj, out, modes, opts.write, bridge).run();
                    0
                }
            }
            _ => {
                eprintln!("{}", cmd_usage("mcp").unwrap());
                2
            }
        },
        "monitor" => cli::run_monitor(&positional, &opts),
        "release" => cli::run_release(&positional, &opts),
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
                "jazyk: docsgen: {} requirements document(s) in {}; diagrams: {} drawn, {} unchanged",
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
            lsp::Lsp::new(proj.root.clone(), out, gs).run();
            0
        }
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
                benchmark::run_goal(&llm, &root, goal)
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
