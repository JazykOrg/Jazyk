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
    s.push_str("  jazyk codegen [entity...]      generate code units from the graph (--lang, default rust)\n");
    s.push_str("  jazyk testgen [entity...]      derive tests from requirements (--lang, default rust)\n");
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
            "--lang" => {
                i += 1;
                opts.lang = args.get(i).cloned();
            }
            "--verbose" | "-v" => opts.verbose = true,
            "--quiet" | "-q" => opts.quiet = true,
            "--write" => opts.write = true,
            "--force" => opts.force = true,
            s if cmd.is_empty() => cmd = s.to_string(),
            s => positional.push(s.to_string()),
        }
        i += 1;
    }

    if want_help || cmd.is_empty() {
        println!("{}", top_usage());
        std::process::exit(if want_help { 0 } else { 2 });
    }

    let code = match cmd.as_str() {
        "compile" => cli::run_compile(&positional, &opts),
        "check" => cli::run_check(&positional, &opts),
        "watch" => cli::run_watch(&positional, &opts),
        "status" => cli::run_status(&positional, &opts),
        "context" => match positional.first() {
            Some(target) => cli::run_context(&positional[1..], &opts, target),
            None => {
                eprintln!("usage: jazyk context <ent:…|req:…|doc.md#/ref|h:…> [--focus k=n,…] [--budget N]");
                2
            }
        },
        "query" => {
            if positional.is_empty() {
                eprintln!("usage: jazyk query <text>");
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
                eprintln!("usage: jazyk mcp graph [--write]");
                2
            }
        },
        "codegen" => cli::run_codegen(&opts, &positional),
        "testgen" => cli::run_testgen(&opts, &positional),
        "viewer" => cli::run_viewer(&opts),
        "lsp" => {
            let (proj, _llm, out) = cli::resolve(&positional, &opts);
            lsp::Lsp::new(proj.root.clone(), out).run();
            0
        }
        "benchmark" => {
            let (_proj, llm, _out) = cli::resolve(&[], &opts);
            benchmark::run(&llm)
        }
        _ => usage(),
    };
    std::process::exit(code);
}
