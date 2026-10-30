//! Explicit action selection over existing engine operations.
//! See docs/compiler/workflows.md.
use crate::vfs::VPathExt;
use crate::{gen, project::Project, runner::SessionRunner, session::Trace, store::Store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cell::Cell;

thread_local! { static GRAPH_ONLY: Cell<bool> = const { Cell::new(false) }; }
pub(crate) fn graph_only() -> bool {
    GRAPH_ONLY.with(Cell::get)
}
struct GraphScope(bool);
impl GraphScope {
    fn enter() -> Self {
        Self(GRAPH_ONLY.with(|s| s.replace(true)))
    }
}
impl Drop for GraphScope {
    fn drop(&mut self) {
        GRAPH_ONLY.with(|s| s.set(self.0));
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
pub enum Step {
    Compile,
    Bind,
    Generate,
    Build,
    Test,
    Review,
    Task {
        #[serde(rename = "taskId")]
        task_id: String,
    },
}
impl Step {
    fn label(&self) -> String {
        match self {
            Self::Task { task_id } => format!("task:{task_id}"),
            _ => serde_json::to_value(self).unwrap()["action"]
                .as_str()
                .unwrap()
                .into(),
        }
    }
}
pub fn parse(value: &Value) -> Result<Vec<Step>, String> {
    if let Some(rows) = value.as_array() {
        for row in rows {
            if let Some(fields) = row.as_object() {
                if fields
                    .keys()
                    .any(|key| key != "action" && !(key == "taskId" && row["action"] == "task"))
                {
                    return Err("Unknown workflow action field.".into());
                }
            }
        }
    }
    let steps: Vec<Step> =
        serde_json::from_value(value.clone()).map_err(|e| format!("Invalid workflow: {e}"))?;
    if steps.is_empty() || steps.len() > 100 {
        return Err("Select between 1 and 100 actions.".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for step in &steps {
        if !seen.insert(step.label()) {
            return Err("Select each action or task only once.".into());
        }
    }
    Ok(steps)
}
fn graph_ready(proj: &Project, store: &Store) -> Result<(), String> {
    let hashes = crate::control::current_doc_hashes(proj);
    if hashes.len() != store.docs.len()
        || hashes
            .iter()
            .any(|(name, hash)| store.docs.get(name).map(|d| &d.content_hash) != Some(hash))
    {
        return Err("Documents changed. Select Compile documents before generation.".into());
    }
    let control = crate::control::Control::load(proj, &proj.out);
    let board = crate::board::Board::derive(store, proj, &control);
    if board
        .goals
        .iter()
        .any(|g| g.mandatory && !crate::board::LEDGER_KINDS.contains(&g.kind.as_str()))
    {
        return Err(
            "Mandatory graph work remains. Compile documents and resolve its questions first."
                .into(),
        );
    }
    Ok(())
}
fn artifact_exists(store: &Store, gs: &gen::GenSettings) -> Result<(), String> {
    if let Some(build) = gen::Ledger::load(&store.out).build {
        for path in build.produces {
            if !gs.deliverable.join(&path).vis_file() {
                return Err(format!(
                    "Missing product {path}. Select Build product before checking it."
                ));
            }
        }
    }
    Ok(())
}
fn successful(v: &Value) -> bool {
    !["failures", "failed", "failing", "runnerFailed", "stale"]
        .iter()
        .any(|key| v[key].as_u64().unwrap_or(0) > 0)
        && v["ok"] != false
        && v["build"]["ok"] != false
        && v["stopped"].as_str().is_none()
        && !v["rows"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["verdict"] == "none"))
        && !v["skipped"].as_array().is_some_and(|rows| {
            rows.iter().any(|row| {
                row["reason"]
                    .as_str()
                    .is_some_and(|s| s.contains("stale") || s.contains("not bound"))
            })
        })
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub async fn run(
    proj: &Project,
    llm: &crate::llm::Llm,
    steps: &[Step],
    trace: &Trace,
) -> Result<Value, String> {
    // Freeze selected tasks before any generation session can edit their commands.
    let selected_tasks = if steps.iter().any(|s| matches!(s, Step::Task { .. })) {
        crate::task_runtime::load(&proj.root)?.1
    } else {
        vec![]
    };
    for step in steps {
        if let Step::Task { task_id } = step {
            if !selected_tasks.iter().any(|t| &t.id == task_id) {
                return Err(format!("Task {task_id} no longer exists."));
            }
        }
    }
    let mut results = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        if trace.is_cancelled() {
            return Ok(json!({"state":"cancelled", "steps":results}));
        }
        let label = step.label();
        trace.line(
            "workflow",
            &format!("{} · step {} of {}", label, index + 1, steps.len()),
        );
        let result = run_step(proj, llm, step, trace, &selected_tasks).await;
        let (state, result) = match result {
            Ok(value) if successful(&value) => ("done", value),
            Ok(value) => ("failed", value),
            Err(error) => ("failed", json!({"error":error})),
        };
        results.push(json!({"step":step,"state":state,"result":result}));
        if trace.is_cancelled() {
            return Ok(json!({"state":"cancelled", "steps":results}));
        }
        if state == "failed" {
            return Ok(json!({"state":"failed", "steps":results}));
        }
    }
    Ok(json!({"state":"done", "steps":results}))
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
async fn run_step(
    proj: &Project,
    llm: &crate::llm::Llm,
    step: &Step,
    trace: &Trace,
    selected_tasks: &[crate::task_runtime::Task],
) -> Result<Value, String> {
    if matches!(step, Step::Compile) {
        let _scope = GraphScope::enter();
        let report = crate::reconcile::compile(proj, llm, &proj.out, trace).await;
        let store = Store::load(&proj.out);
        graph_ready(proj, &store)?;
        return Ok(json!(report));
    }
    if let Step::Task { task_id } = step {
        let current = crate::task_runtime::load(&proj.root)?.1;
        let expected = selected_tasks.iter().find(|t| &t.id == task_id).unwrap();
        if !current.iter().any(|t| {
            &t.id == task_id
                && serde_json::to_value(t).ok() == serde_json::to_value(expected).ok()
        }) {
            return Err(format!(
                "Task {task_id} changed. Review the workflow before running it again."
            ));
        }
        let (id, result) = crate::task_runtime::execute_with_key(&proj.root, task_id, "workflow")?;
        if result["status"] != "succeeded" || result["complete"] != true {
            return Err(format!("Task {task_id} did not succeed. Inspect run {id}."));
        }
        return Ok(json!({"runId":id,"result":result}));
    }
    let store = Store::load(&proj.out);
    let gs = gen::GenSettings::resolve(proj);
    if matches!(step, Step::Bind | Step::Generate) {
        graph_ready(proj, &store)?;
    }
    if matches!(step, Step::Generate) && !crate::bind::pending(&store, &gs).is_empty() {
        return Err(
            "Tests need binding. Select Generate tests before Generate product.".into(),
        );
    }
    if matches!(step, Step::Build) {
        gen::run_build(&proj.out, &gs, trace, "workflow")?;
        return Ok(json!({"ok":true}));
    }
    if matches!(step, Step::Test | Step::Review) {
        artifact_exists(&store, &gs)?;
    }
    if matches!(step, Step::Test) {
        let targets = gen::Ledger::load(&proj.out)
            .requirements
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        return crate::verify::run_selected_after_build(&store, &gs, &targets);
    }
    let stage = if matches!(step, Step::Review) {
        "verify"
    } else {
        "generate"
    };
    let _lease = crate::control::begin_internal_build(proj, &proj.out, stage)?;
    let runner: Box<dyn SessionRunner> = if crate::vfs::is_scoped() {
        Box::new(crate::inprocess::InProcessRunner::start(
            proj, llm, &proj.out,
        )?)
    } else {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Box::new(crate::acp::runner::AcpRunner::start(proj, llm, &proj.out)?)
        }
        #[cfg(target_arch = "wasm32")]
        {
            Box::new(crate::inprocess::InProcessRunner::start(
                proj, llm, &proj.out,
            )?)
        }
    };
    runner.set_build_token(Some(format!("internal-{}", crate::sys::process_id())));
    match step {
        Step::Bind => crate::bind::run_all(&store, runner.as_ref(), &gs, &[], trace).await,
        Step::Generate => {
            gen::run_selected(&store, runner.as_ref(), &gs, &[], false, trace, false).await
        }
        Step::Review => crate::verify::run_without_build(
            &store,
            runner.as_ref(),
            &gs,
            &[],
            Some("llm"),
            true,
            trace,
            false,
        ).await,
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_empty_unknown_duplicate_and_malformed_actions() {
        for v in [
            json!([]),
            json!([{"action":"publish"}]),
            json!([{"action":"task"}]),
            json!([{"action":"test"},{"action":"test"}]),
            json!([{"action":"compile","force":true}]),
        ] {
            assert!(parse(&v).is_err());
        }
        assert_eq!(
            parse(&json!([{"action":"compile"},{"action":"task","taskId":"build"}]))
                .unwrap()
                .len(),
            2
        );
    }
    #[test]
    fn failed_evidence_blocks_following_steps() {
        assert!(!successful(&json!({"failed":1})));
        assert!(!successful(&json!({"runnerFailed":1})));
        assert!(!successful(&json!({"build":{"ok":false}})));
        assert!(successful(&json!({"passed":3,"failed":0})));
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn missing_artifact_stops_before_build_and_preserves_files() {
        let config = "version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Main'\ndocs = ['docs/**']\nsource = ['src/**']\n";
        let ledger = "build:\n  run: 'this-command-must-never-run'\n  produces: ['missing.pdf']\n";
        let files = [
            (".jazyk/jazyk.toml", config),
            (".jazyk/ds_main/gen/ledger.yaml", ledger),
            ("src/book.md", "Keep this manuscript"),
        ]
        .into_iter()
        .map(|(p, text)| (p.into(), text.as_bytes().to_vec()))
        .collect();
        let mut workspace = crate::hosted::Workspace::new(files).unwrap();
        let reply = workspace
            .request(
                "POST",
                "/api/jobs",
                &json!({"kind":"workflow", "setId":"ds_main",
            "steps":[{"action":"test"},{"action":"build"}]})
                .to_string(),
                &crate::hosted::Execution::default(),
            )
            .unwrap();
        let result: Value = serde_json::from_slice(&reply.body).unwrap();
        assert_eq!(reply.status, 422, "{result}");
        assert_eq!(result["report"]["steps"].as_array().unwrap().len(), 1);
        assert!(result["report"]["steps"][0]["result"]["error"]
            .as_str()
            .unwrap()
            .contains("Missing product"));
        assert_eq!(workspace.snapshot()["src/book.md"], b"Keep this manuscript");
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cancellation_runs_no_action_and_no_model() {
        let config = "version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Main'\ndocs = ['docs/**']\nsource = []\n";
        let mut workspace = crate::hosted::Workspace::new(
            [(".jazyk/jazyk.toml".into(), config.as_bytes().to_vec())].into(),
        )
        .unwrap();
        let execution = crate::hosted::Execution {
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            ..Default::default()
        };
        let reply = workspace
            .request(
                "POST",
                "/api/jobs",
                &json!({"kind":"workflow", "setId":"ds_main",
            "steps":[{"action":"compile"},{"action":"generate"}]})
                .to_string(),
                &execution,
            )
            .unwrap();
        let result: Value = serde_json::from_slice(&reply.body).unwrap();
        assert_eq!(result["report"]["state"], "cancelled", "{result}");
        assert_eq!(result["report"]["steps"], json!([]));
    }
    #[test]
    fn graph_scope_restores_after_unwind() {
        let _ = std::panic::catch_unwind(|| {
            let _scope = GraphScope::enter();
            assert!(graph_only());
            panic!("stop");
        });
        assert!(!graph_only());
    }
}
