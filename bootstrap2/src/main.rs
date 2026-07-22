mod benchmark;
mod cli;
mod context;
mod docsgen;
mod gen;
mod jsonrpc;
mod llm;
mod lsp;
mod mcp;
mod md;
mod model;
mod parallel;
mod project;
mod reconcile;
mod store;
mod tools;
mod turn;
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
    s.push_str("jazyk — natural language compiler (turn-based)\n\n");
    s.push_str("usage:\n");
    s.push_str("  jazyk compile [path...]        reconcile the graph with the documents\n");
    s.push_str("  jazyk check [path...]          compile, exit non-zero on error diagnostics (CI)\n");
    s.push_str("  jazyk watch [path...]          recompile on change\n");
    s.push_str("  jazyk status                   summarize the last build\n");
    s.push_str("  jazyk context <target>         print a context pack (ent:…, req:…, doc.md#/ref, or h:… handle)\n");
    s.push_str("  jazyk query <text>             search entities\n");
    s.push_str("  jazyk gen [entity...]          generate the deliverable and its tests from the graph (--force)\n");
    s.push_str("  jazyk test [target...]         run verification (--kind programmatic|llm, --list, --audit, --force)\n");
    s.push_str("  jazyk docsgen                  render per-entity requirements documents on demand\n");
    s.push_str("  jazyk viewer [--out FILE]      render the graph to a self-contained HTML page\n");
    s.push_str("  jazyk mcp graph [--write]      the graph MCP server over stdio\n");
    s.push_str("  jazyk lsp                      language server over stdio (read-only; compile or watch rebuilds)\n");
    s.push_str("  jazyk benchmark                grade the configured model under both codecs\n");
    s.push_str("\noptions: --llm-base-url URL  --model M  --api-key K  --out DIR\n");
    s.push_str("         --verbose, -v   full context packs and payloads in the trace\n");
    s.push_str("         --quiet, -q     only the final summary\n");
    s.push_str("         --focus k=n,…   context hop quotas (parents, mentions, requirements)\n");
    s.push_str("         --budget N      context size budget in characters\n");
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
        "compile" => format!(
            "usage: jazyk compile [path...]\n\n\
             Reconcile the graph with the documents, running turns to a fixed point.\n\
             Explicit paths skip project discovery and run ad hoc on those files.\n\n\
             options:\n  \
             --verbose, -v   full context packs and payloads in the trace\n  \
             --quiet, -q     only the final summary\n\
             {}\n\n\
             exit: 0 on convergence, non-zero when work was parked",
            COMMON_LLM
        ),
        "check" => format!(
            "usage: jazyk check [path...]\n\n\
             Compile, then exit non-zero if open diagnostics of severity error exist.\n\
             The CI gate.\n\n\
             options:\n  \
             --verbose, -v   full context packs and payloads in the trace\n  \
             --quiet, -q     only the final summary\n\
             {}",
            COMMON_LLM
        ),
        "watch" => format!(
            "usage: jazyk watch [path...]\n\n\
             Recompile on file change. Event bursts debounce, and a fingerprint over the\n\
             matched documents decides whether a build runs. An incomplete build retries\n\
             on its own with backoff until a file change resets it.\n\n\
             options:\n  \
             --verbose, -v   full context packs and payloads in the trace\n  \
             --quiet, -q     only the final summary\n\
             {}",
            COMMON_LLM
        ),
        "status" => format!(
            "usage: jazyk status\n\n\
             Summarize the last build: generation counter, coverage percentage, open\n\
             diagnostics by severity, parked work.\n\n\
             {}",
            COMMON_OUT
        ),
        "context" => format!(
            "usage: jazyk context <ent:…|req:…|doc.md#/ref|h:…>\n\n\
             Print the rendered context pack for a target, with its expansion handles.\n\
             What this prints is exactly what a turn sees.\n\n\
             options:\n  \
             --focus k=n,…   context hop quotas (parents, mentions, requirements)\n  \
             --budget N      context size budget in characters (default 12000)\n\
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
             Generate the deliverable and its tests from the graph, and record the\n\
             manifest in the ledger. With no arguments, cover every entity that has at\n\
             least one requirement, leaf entities first, skipping entities whose facts\n\
             are unchanged.\n\n\
             options:\n  \
             --force   regenerate even when facts are unchanged\n\
             {}",
            COMMON_LLM
        ),
        "test" => format!(
            "usage: jazyk test [target...]\n\n\
             Run verification over the generation ledger. Entity ids select their\n\
             requirements' rows; requirement ids select rows directly.\n\n\
             options:\n  \
             --kind programmatic|llm   only rows of this kind\n  \
             --list                    print the derived status table without running\n  \
             --audit                   rebuild the ledger from the artifact markers\n  \
             --force                   also rerun verified rows\n\
             {}\n\n\
             exit: 0 when every targeted row is verified, 1 otherwise",
            COMMON_LLM
        ),
        "docsgen" => format!(
            "usage: jazyk docsgen\n\n\
             Render the per-entity requirements documents into <out>/docsgen/ without\n\
             compiling.\n\n\
             {}",
            COMMON_OUT
        ),
        "viewer" => "usage: jazyk viewer [--out FILE]\n\n\
             Render the graph to one self-contained HTML page. An --out ending in .html\n\
             names the file (default <out>/graph.html); otherwise --out is the out\n\
             directory."
            .to_string(),
        "mcp" => format!(
            "usage: jazyk mcp graph [--write]\n\n\
             Serve the graph MCP server over stdio. Read tools by default; --write adds\n\
             the write tools.\n\n\
             {}",
            COMMON_OUT
        ),
        "lsp" => format!(
            "usage: jazyk lsp\n\n\
             Language server over stdio. Read-only: serves the last committed graph, and\n\
             a compile or watch rebuild refreshes it.\n\n\
             {}",
            COMMON_OUT
        ),
        "benchmark" => format!(
            "usage: jazyk benchmark\n\n\
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
            "--focus" => {
                i += 1;
                opts.focus = args.get(i).cloned();
            }
            "--budget" => {
                i += 1;
                opts.budget = args.get(i).and_then(|s| s.parse::<usize>().ok());
            }
            "--kind" => {
                i += 1;
                opts.kind = args.get(i).cloned();
            }
            "--verbose" | "-v" => opts.verbose = true,
            "--quiet" | "-q" => opts.quiet = true,
            "--write" => opts.write = true,
            "--force" => opts.force = true,
            "--list" => opts.list = true,
            "--audit" => opts.audit = true,
            s if cmd.is_empty() => cmd = s.to_string(),
            s => positional.push(s.to_string()),
        }
        i += 1;
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
        "compile" => cli::run_compile(&positional, &opts),
        "check" => cli::run_check(&positional, &opts),
        "watch" => cli::run_watch(&positional, &opts),
        "status" => cli::run_status(&positional, &opts),
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
            Some("graph") => {
                let (proj, _llm, out) = cli::resolve(&[], &opts);
                mcp::McpServer::new(proj, out, opts.write).run();
                0
            }
            _ => {
                eprintln!("{}", cmd_usage("mcp").unwrap());
                2
            }
        },
        "gen" => cli::run_gen(&opts, &positional),
        "test" => cli::run_test(&opts, &positional),
        "codegen" | "testgen" => {
            eprintln!("jazyk: `{}` is deprecated; generation is one workflow now, use `jazyk gen` (and `jazyk test` to verify)", cmd);
            cli::run_gen(&opts, &positional)
        }
        "docsgen" => {
            let (proj, _llm, out) = cli::resolve(&[], &opts);
            let store = store::Store::load(&out);
            let n = docsgen::write_all(&store, &gen::GenSettings::resolve(&proj, &out));
            println!("jazyk: docsgen — {} requirements document(s) in {}", n, out.join("docsgen").display());
            0
        }
        "viewer" => cli::run_viewer(&opts),
        "lsp" => {
            let (proj, _llm, out) = cli::resolve(&positional, &opts);
            let gs = gen::GenSettings::resolve(&proj, &out);
            lsp::Lsp::new(proj.root.clone(), out, gs).run();
            0
        }
        "benchmark" => {
            let (_proj, llm, out) = cli::resolve(&[], &opts);
            benchmark::run(&llm, &out)
        }
        _ => usage(),
    };
    std::process::exit(code);
}
