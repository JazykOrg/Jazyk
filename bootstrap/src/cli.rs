// CLI command implementations. Mirrors docs/frontends/cli.md.
use crate::llm::Llm;
use crate::project::{self, Project};
use crate::reconcile;
use crate::session::{Trace, TraceLevel};
use std::path::PathBuf;

mod build;
#[cfg(test)]
mod build_tests;
mod control;
#[cfg(test)]
mod control_tests;
mod init;
mod inspect;
#[cfg(test)]
mod inspect_tests;
mod ledger;
#[cfg(test)]
mod tests;

pub use build::{run_check, run_compile, run_watch};
pub use control::{run_answer, run_monitor, run_release, run_spend};
pub(crate) use init::{init_scaffold, INIT_TOML};
pub use init::{run_acp_install, run_gui, run_init, run_sets};
pub use inspect::{
    run_context, run_explain, run_preview, run_query, run_ripple, run_status, run_viewer,
    shape_line,
};
pub use ledger::{run_decompile, run_gen, run_test};

#[derive(Clone, Default)]
pub struct Options {
    pub set: Option<String>,
    pub docs: Vec<String>,
    pub source: Vec<String>,
    pub roots: Vec<String>,
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub llm_api: Option<String>,
    pub reasoning_summary: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub out: Option<String>,
    pub verbose: bool,
    pub quiet: bool,
    // `jazyk compile --record`: a graph snapshot per batch (docs/compiler/sessions.md#snapshots).
    pub record: bool,
    pub write: bool,
    // `jazyk context`: neighbor depth and the handles to follow before printing.
    pub depth: Option<u32>,
    pub expand: Vec<String>,
    // `jazyk ripple --back`: walk causes instead of consequences.
    pub back: bool,
    pub force: bool,
    pub kind: Option<String>,
    pub list: bool,
    pub audit: bool,
    pub port: Option<u16>,
    pub no_open: bool,
    pub watch: bool,
    pub gui_url: Option<String>,
    pub no_token: bool,
    pub dev: bool,
    pub mcp: Option<String>,
    pub json: bool,
    // `jazyk status --costs`: the journal's cost report (docs/frontends/cli.md#jazyk-status).
    pub costs: bool,
    // `jazyk spend <generation> <tokens> --cached N`: the cached input among the
    // reported tokens (docs/frontends/cli.md#jazyk-spend).
    pub cached: Option<u64>,
    pub once: bool,
    // ACP: the agent profile (--agent), and the flags of bridge-spawned MCP servings
    // (docs/frontends/mcp.md#mcp-into-acp-sessions).
    pub agent: Option<String>,
    pub acp_ide: Option<String>,
    // `jazyk setup --remove`: setup names, or `all`.
    pub remove: Option<String>,
    pub ephemeral: bool,
    pub only: Option<String>,
    pub build_token: Option<String>,
    pub cancel_file: Option<String>,
    pub serve_files: bool,
    pub edit_sink: Option<String>,
    pub packaged: bool,
    // `jazyk benchmark --project <dir> --goal <id>`: one goal's session from a copy
    // of a real project (docs/benchmark/benchmark.md#snippets-from-a-real-project).
    pub project: Option<String>,
    pub goal: Option<String>,
    // `jazyk compile --sessions N`: at most N sessions this run.
    pub sessions: Option<usize>,
    // `jazyk compile --select PATH[:A-B]`: a selected build
    // (docs/compiler/compilation.md#selected-builds).
    pub select: Vec<String>,
    // `jazyk answer <id> --option N | --text "..."`: the reply to a prompt
    // (docs/frontends/cli.md#jazyk-answer).
    pub option: Option<usize>,
    pub text: Option<String>,
}

// Resolve the project (walking up to jazyk.toml, or ad hoc with explicit paths), the LLM
// (flag → env → project → global config → default), and the out directory.
pub fn resolve(paths: &[String], opts: &Options) -> (Project, Llm, PathBuf) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = project::find_root(&cwd).unwrap_or_else(|| cwd.clone());
    let mut proj = if let Some(set) = opts.set.as_deref() {
        Project::load_set(&root, set).unwrap_or_else(|(_, error)| Project {
            root: root.clone(),
            config_error: Some(error),
            docs_glob: vec![],
            ..Project::default()
        })
    } else {
        Project::load(&root)
    };
    let _ = paths; // Path filters choose sets in run_compile; never truncate a graph's input.

    let global = project::load_global_llm();
    let llm = resolve_llm(opts, &proj.llm, &global, |name| std::env::var(name).ok());

    let out = opts
        .out
        .clone()
        .map(|o| {
            let pb = PathBuf::from(o);
            if pb.is_absolute() {
                pb
            } else {
                cwd.join(pb)
            }
        })
        .unwrap_or_else(|| proj.out.clone());
    if opts.out.is_some()
        && (opts.set.is_none() || (out.starts_with(root.join(".jazyk")) && out != proj.out))
    {
        proj.config_error = Some(
            "--out requires --set and cannot target project metadata or another set's store".into(),
        );
    }
    // The out directory is never doc input; doc_files skips it by path.
    proj.out = out.clone();
    (proj, llm, out)
}

// Per-field precedence: CLI flag → env → project [llm] → global config → built-in default.
// Mirrors docs/compiler/project-settings.md#llm. `env` is injected for testability.
// The refusal of a command that works on one set whose store is newer than this
// binary: printed, exit 2, before the command reads or writes anything.
// Mirrors docs/compiler/graph.md#store-version.
pub fn refuse_newer_store(opts: &Options) -> Option<i32> {
    let (_proj, _llm, out) = resolve(&[], opts);
    let why = crate::store::newer_store(&out)?;
    eprintln!("jazyk: {}", why);
    Some(2)
}

pub(crate) fn resolve_llm(
    opts: &Options,
    proj: &project::LlmSettings,
    global: &project::GlobalLlm,
    env: impl Fn(&str) -> Option<String>,
) -> Llm {
    let base_url = opts
        .base_url
        .clone()
        .or_else(|| env("JAZYK_LLM_BASE_URL"))
        .or_else(|| proj.base_url.clone())
        .or_else(|| global.base_url.clone())
        .unwrap_or_else(|| "http://localhost:11434/v1".to_string());
    let model = opts
        .model
        .clone()
        .or_else(|| env("JAZYK_MODEL"))
        .or_else(|| proj.model.clone())
        .or_else(|| global.model.clone())
        .unwrap_or_else(|| "llama3.1".to_string());
    // The env tier reads the variable named by api_key_env (itself resolved
    // project → global → default), then literal keys follow the same order.
    let api_key_env = proj
        .api_key_env
        .clone()
        .or_else(|| global.api_key_env.clone())
        .unwrap_or_else(|| "JAZYK_API_KEY".to_string());
    let api_key = opts
        .api_key
        .clone()
        .or_else(|| env(&api_key_env))
        .or_else(|| proj.api_key.clone())
        .or_else(|| global.api_key.clone())
        .unwrap_or_default();
    let temperature = env("JAZYK_TEMPERATURE")
        .and_then(|s| s.parse::<f64>().ok())
        .or(proj.temperature)
        .or(global.temperature)
        .or(Some(0.0))
        .filter(|t| *t >= 0.0);
    // The trace is attached per run by whoever starts the work (`with_trace`).
    Llm {
        api: opts
            .llm_api
            .clone()
            .or_else(|| env("JAZYK_LLM_API"))
            .or_else(|| proj.api.clone())
            .or_else(|| global.api.clone())
            .unwrap_or_else(|| "chat_completions".into()),
        reasoning_summary: opts
            .reasoning_summary
            .clone()
            .or_else(|| env("JAZYK_REASONING_SUMMARY"))
            .or_else(|| proj.reasoning_summary.clone())
            .or_else(|| global.reasoning_summary.clone()),
        base_url,
        model,
        api_key,
        temperature,
        trace: None,
    }
}

// Spawn the run's ACP runner (docs/frontends/acp.md#worker-sessions), or explain why not.
fn runner_for(
    proj: &Project,
    llm: &Llm,
    out: &std::path::Path,
) -> Result<crate::acp::runner::AcpRunner, String> {
    crate::acp::runner::AcpRunner::start(proj, llm, out)
}

fn trace_for(opts: &Options) -> Trace {
    Trace::stderr(if opts.quiet {
        TraceLevel::Quiet
    } else if opts.verbose {
        TraceLevel::Verbose
    } else {
        TraceLevel::Normal
    })
}

// The build's last line: the verdict with its counts, e.g. `converged, 2 blocked,
// 1 optional advised`. Mirrors docs/frontends/cli.md#jazyk-compile.
fn verdict_line(r: &reconcile::BuildReport) -> String {
    r.verdict_line()
}

// Whether a set's documents include `path` (relative to `cwd`), or a file under it
// when it names a directory.
pub(crate) fn set_claims(
    workspace: &crate::document_sets::Workspace,
    set: &crate::document_sets::DocumentSet,
    cwd: &std::path::Path,
    path: &str,
) -> bool {
    use crate::document_sets::included;
    cwd.join(path)
        .strip_prefix(&workspace.root)
        .ok()
        .is_some_and(|rel| {
            let rel = rel.to_string_lossy().replace('\\', "/");
            included(&set.docs, &rel)
                || workspace
                    .files()
                    .iter()
                    .any(|f| f.starts_with(&format!("{}/", rel)) && included(&set.docs, f))
        })
}
