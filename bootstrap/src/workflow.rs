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
fn mandatory_graph_goal(goal: &crate::model::Goal) -> bool {
    goal.mandatory && !crate::board::LEDGER_KINDS.contains(&goal.kind.as_str())
}

// Outside a selection nothing holds the workflow (docs/compiler/workflows.md#execution).
fn graph_complete(board: &crate::board::Board) -> Result<(), String> {
    let blockers: Vec<_> = board
        .goals
        .iter()
        .filter(|goal| {
            mandatory_graph_goal(goal)
                && !board.outside.contains(&goal.id)
                && matches!(
                    goal.state,
                    crate::model::GoalState::Open
                        | crate::model::GoalState::Parked
                        | crate::model::GoalState::Failed { .. }
                )
        })
        .collect();
    if !blockers.is_empty() {
        let mut details: Vec<String> = blockers
            .iter()
            .take(5)
            .map(|goal| {
                let state = match &goal.state {
                    crate::model::GoalState::Parked => board
                        .parked_reasons
                        .get(&goal.id)
                        .map(|reason| format!("parked: {reason}"))
                        .unwrap_or_else(|| "parked".into()),
                    crate::model::GoalState::Failed { reason } => format!("failed: {reason}"),
                    _ => "open".into(),
                };
                format!("{} ({state})", goal.id)
            })
            .collect();
        if blockers.len() > details.len() {
            details.push(format!("{} more", blockers.len() - details.len()));
        }
        return Err(format!("Mandatory graph work remains: {}. Inspect these goals and resolve their blockers before continuing.", details.join("; ")));
    }
    Ok(())
}

fn graph_ready(proj: &Project, store: &Store) -> Result<(), String> {
    let hashes = crate::control::current_doc_hashes(proj);
    let changed = match crate::selection::current() {
        // Only the selected documents must be compiled.
        Some(items) => hashes.iter().any(|(doc, hash)| {
            items
                .iter()
                .any(|i| doc == &i.path || doc.starts_with(&format!("{}/", i.path)))
                && store.docs.get(doc).map(|r| &r.content_hash) != Some(hash)
        }),
        None => {
            hashes.len() != store.docs.len()
                || hashes
                    .iter()
                    .any(|(name, hash)| store.docs.get(name).map(|d| &d.content_hash) != Some(hash))
        }
    };
    if changed {
        return Err("Documents changed. Select Compile documents before generation.".into());
    }
    let control = crate::control::Control::load(proj, &proj.out);
    let board = crate::board::Board::derive(store, proj, &control);
    graph_complete(&board)?;
    if board.goals.iter().any(|goal| {
        mandatory_graph_goal(goal)
            && !board.outside.contains(&goal.id)
            && matches!(&goal.state, crate::model::GoalState::Blocked { on } if !on.starts_with("human"))
    }) {
        return Err("Mandatory graph work is waiting on a release. Compile documents before continuing.".into());
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
    v["error"].is_null()
        && !["failures", "failed", "failing", "runnerFailed", "stale"]
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
                row["reason"].as_str().is_some_and(|s| {
                    s.contains("stale")
                        || s.contains("not bound")
                        || s.starts_with("source-selection-changed:")
                })
            })
        })
}

#[derive(Serialize, Deserialize)]
struct Progress {
    version: u32,
    steps: Vec<Step>,
    selected_tasks: Vec<crate::task_runtime::Task>,
    next: usize,
    active: Option<usize>,
    results: Vec<Value>,
    finished: Option<String>,
}

impl Progress {
    fn enabled() -> bool {
        cfg!(target_arch = "wasm32") || crate::vfs::is_scoped()
    }

    fn load(key: &str, steps: &[Step]) -> Result<Option<Self>, String> {
        if !Self::enabled() {
            return Ok(None);
        }
        let Some(value) = crate::session_recovery::parent(key)? else {
            return Ok(None);
        };
        let progress: Self = serde_json::from_value(value)
            .map_err(|error| format!("checkpoint-incompatible: workflow continuation: {error}"))?;
        if progress.version != 1
            || progress.steps != steps
            || progress.next > steps.len()
            || progress.results.len() != progress.next
            || progress
                .active
                .is_some_and(|index| index != progress.next || index >= steps.len())
        {
            return Err("checkpoint-incompatible: workflow selection or progress changed".into());
        }
        Ok(Some(progress))
    }

    fn fresh(proj: &Project, steps: &[Step]) -> Result<Self, String> {
        let selected_tasks = if steps.iter().any(|s| matches!(s, Step::Task { .. })) {
            crate::task_runtime::load(&proj.root)?.1
        } else {
            vec![]
        };
        for step in steps {
            if let Step::Task { task_id } = step {
                if !selected_tasks.iter().any(|task| &task.id == task_id) {
                    return Err(format!("Task {task_id} no longer exists."));
                }
            }
        }
        Ok(Self {
            version: 1,
            steps: steps.to_vec(),
            selected_tasks,
            next: 0,
            active: None,
            results: vec![],
            finished: None,
        })
    }

    #[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
    async fn save(&self, key: &str) -> Result<(), String> {
        if !Self::enabled() {
            return Ok(());
        }
        crate::session_recovery::set_parent(
            key,
            serde_json::to_value(self)
                .map_err(|error| format!("checkpoint-failed: workflow continuation: {error}"))?,
        );
        crate::session_recovery::checkpoint_parent()
            .await
            .map_err(|error| {
                if crate::session_recovery::is_interruption(&error) {
                    error
                } else {
                    format!("checkpoint-failed: {error}")
                }
            })
    }

    fn result(&self, state: &str) -> Value {
        json!({"state":state,"steps":self.results})
    }
}

fn interruption(value: &Value) -> Option<String> {
    ["suspendReason", "stopped", "error"]
        .iter()
        .filter_map(|field| value[*field].as_str())
        .find(|reason| crate::session_recovery::is_interruption(reason))
        .map(str::to_string)
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub async fn run(
    proj: &Project,
    llm: &crate::llm::Llm,
    steps: &[Step],
    trace: &Trace,
) -> Result<Value, String> {
    let key = crate::session_recovery::command_key(&format!("workflow:{}", proj.out.display()));
    let mut progress = match Progress::load(&key, steps)? {
        Some(progress) => progress,
        None => Progress::fresh(proj, steps)?,
    };
    if let Some(state) = &progress.finished {
        return Ok(progress.result(state));
    }
    // The compile action leaves the task list alone for later actions; the
    // workflow sweeps once when it ends (docs/consumers/gen.md#session-tasks-are-swept-when-unused).
    let _defer = crate::task_runtime::DeferSweep::enter();
    while progress.next < steps.len() {
        if trace.is_cancelled() {
            progress.finished = Some("cancelled".into());
            progress.save(&key).await?;
            return Ok(progress.result("cancelled"));
        }
        progress.active = Some(progress.next);
        progress.save(&key).await?;
        if crate::session_recovery::suspension_requested() {
            let mut result = progress.result("suspended");
            result["suspendReason"] = json!("suspended");
            return Ok(result);
        }
        let step = &steps[progress.next];
        trace.line(
            "workflow",
            &format!(
                "{} · step {} of {}",
                step.label(),
                progress.next + 1,
                steps.len()
            ),
        );
        let result = run_step(proj, llm, step, trace, &progress.selected_tasks).await;
        // A selection steers only the first action (docs/frontends/gui.md#selected-builds).
        if let Some(h) = crate::selection::handle() {
            h.seal();
        }
        let reason = match &result {
            Ok(value) => interruption(value),
            Err(error) if crate::session_recovery::is_interruption(error) => Some(error.clone()),
            _ => None,
        };
        if let Some(reason) = reason {
            progress.save(&key).await?;
            let mut result = progress.result("suspended");
            result["suspendReason"] = json!(reason);
            return Ok(result);
        }
        let (state, result) = match result {
            Ok(value) if successful(&value) => ("done", value),
            Ok(value) => ("failed", value),
            Err(error) => ("failed", json!({"error":error})),
        };
        progress
            .results
            .push(json!({"step":step,"state":state,"result":result}));
        progress.next += 1;
        progress.active = None;
        if trace.is_cancelled() {
            progress.finished = Some("cancelled".into());
        } else if state == "failed" {
            progress.finished = Some("failed".into());
        } else if progress.next == steps.len() {
            progress.finished = Some("done".into());
        }
        progress.save(&key).await?;
        if let Some(state) = &progress.finished {
            let mut result = progress.result(state);
            if state != "cancelled" {
                if let Ok(_lease) = crate::control::begin_internal_build(proj, &proj.out, "sweep") {
                    let retired = crate::reconcile::sweep_tasks(proj, &proj.out, trace);
                    if !retired.is_empty() {
                        result["retiredTasks"] = json!(retired);
                    }
                }
            }
            return Ok(result);
        }
    }
    Ok(progress.result("done"))
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
        let mut result = json!(report);
        if report.suspend_reason.is_none() {
            // Admission refusals return before the compiler derives a counted
            // verdict. The previously saved graph may already be complete.
            if report.verdict == "incomplete" {
                result["error"] =
                    json!("Compilation could not start. Inspect the build trace for the refusal.");
            } else {
                let store = Store::load(&proj.out);
                let control = crate::control::Control::load(proj, &proj.out);
                let board = crate::board::Board::derive(&store, proj, &control);
                if let Err(error) = graph_complete(&board) {
                    result["error"] = json!(error);
                }
            }
        }
        return Ok(result);
    }
    if let Step::Task { task_id } = step {
        let current = crate::task_runtime::load(&proj.root)?.1;
        let expected = selected_tasks.iter().find(|t| &t.id == task_id).unwrap();
        if !current.iter().any(|t| {
            &t.id == task_id && serde_json::to_value(t).ok() == serde_json::to_value(expected).ok()
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
    // A selection narrows the ledger actions to what is anchored in it.
    let selected = crate::selection::current().map(|items| {
        let r = crate::selection::Resolved::new(&store, &items);
        (r.entities(), r.requirements())
    });
    let (entities, requirements) = selected.clone().unwrap_or_default();
    if let Some((e, r)) = &selected {
        let empty = match step {
            Step::Generate => e.is_empty(),
            Step::Bind | Step::Test | Step::Review => r.is_empty(),
            _ => false,
        };
        if empty {
            trace.line("workflow", "Nothing in the selection");
            return Ok(json!({"skipped": "Nothing in the selection"}));
        }
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
            .filter(|r| selected.is_none() || requirements.contains(r))
            .cloned()
            .collect::<Vec<_>>();
        if targets.is_empty() && selected.is_some() {
            trace.line("workflow", "Nothing in the selection");
            return Ok(json!({"skipped": "Nothing in the selection"}));
        }
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
    runner.set_build_token(Some(_lease.token().to_owned()));
    match step {
        Step::Bind => {
            crate::bind::run_all(&store, runner.as_ref(), &gs, &requirements, trace).await
        }
        Step::Generate => {
            gen::run_selected(&store, runner.as_ref(), &gs, &entities, false, trace, false).await
        }
        Step::Review => {
            crate::verify::run_without_build(
                &store,
                runner.as_ref(),
                &gs,
                &requirements,
                Some("llm"),
                true,
                trace,
                false,
            )
            .await
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_arch = "wasm32"))]
    use std::path::Path;

    #[cfg(not(target_arch = "wasm32"))]
    fn pending_workspace(
        requirements: usize,
        criteria: usize,
        question: bool,
    ) -> crate::hosted::Snapshot {
        use crate::model::*;
        let config = "version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Main'\ndocs = ['docs/**']\nroots = ['docs/main.md']\nsource = ['src/**', 'tests/**']\n";
        let document = format!(
            "# Site\n\n{}\n",
            (1..=requirements)
                .map(|index| format!("Site shows item {index}."))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let files = std::collections::BTreeMap::from([
            (".jazyk/jazyk.toml".into(), config.as_bytes().to_vec()),
            ("docs/main.md".into(), document.as_bytes().to_vec()),
        ]);
        let fs = crate::vfs::Scope::enter(&files);
        let project = crate::document_sets::Workspace::load(Path::new("/project"))
            .unwrap()
            .resolve(None)
            .unwrap();
        let mut store = Store {
            out: project.out.clone(),
            ..Default::default()
        };
        store.sync_docs(&crate::reconcile::parse_all(&project).0);
        store.status.generation = 33;
        store.status.changes.clear();
        for record in store.docs.values_mut() {
            for section in record.sections.keys() {
                record.coverage.insert(
                    section.clone(),
                    Coverage {
                        state: "covered".into(),
                        note: Some("Every item statement is recorded.".into()),
                        claimed_by: None,
                    },
                );
            }
        }
        let source = |index: usize| SourceRef {
            doc: "docs/main.md".into(),
            section: "/site".into(),
            quote: format!("Site shows item {index}."),
        };
        store.graph.entities.insert(
            "ent:site".into(),
            Entity {
                name: "Site".into(),
                mentions: vec![source(1)],
                ..Default::default()
            },
        );
        for index in 1..=requirements {
            store.graph.requirements.insert(
                format!("req:site-{index}"),
                Requirement {
                    statement: format!("Site shows item {index}."),
                    entities: vec!["ent:site".into()],
                    source: Some(source(index)),
                    provenance: Some(Provenance::Quote(source(index))),
                    ..Default::default()
                },
            );
        }
        for index in 1..=criteria {
            let rid = format!("req:site-{}", (index - 1) % requirements + 1);
            let cid = format!("crit:site-{index}");
            store.graph.criteria.insert(
                cid.clone(),
                Criterion {
                    requirement: rid.clone(),
                    statement: format!("Item {index} is visible."),
                    kind: "positive".into(),
                    provenance: Some(Provenance::Derived {
                        from: vec![rid],
                        reasoning: "A visible item witnesses its requirement.".into(),
                    }),
                    ..Default::default()
                },
            );
            store.graph.diagnostics.insert(
                format!("diag:ratification-pending-{index}"),
                Diagnostic {
                    goal: None,
                    rule: "ratification-pending".into(),
                    severity: "info".into(),
                    lifecycle: "open".into(),
                    subjects: vec![cid],
                    message: format!("Accept criterion {index}."),
                    created: Some("g33".into()),
                    prompt: Some(DiagnosticPrompt {
                        question: format!("Accept criterion {index}?"),
                        options: vec![],
                        freeform: true,
                    }),
                    reasoning: None,
                    triage: None,
                    answer: None,
                    updated: None,
                },
            );
        }
        if question {
            let id = "diag:contradiction-1";
            store.graph.diagnostics.insert(
                id.into(),
                Diagnostic {
                    goal: None,
                    rule: "contradiction".into(),
                    severity: "error".into(),
                    lifecycle: "open".into(),
                    subjects: vec!["req:site-1".into()],
                    message: "The item wording needs a human decision.".into(),
                    prompt: Some(DiagnosticPrompt {
                        question: "Which item wording holds?".into(),
                        options: vec![],
                        freeform: true,
                    }),
                    reasoning: None,
                    triage: None,
                    answer: None,
                    created: None,
                    updated: None,
                },
            );
            store.status.changes.push(ChangeRecord::new(
                33,
                1,
                0,
                crate::store::CHANGE_PROMPT_UNANSWERED,
                id,
                "report_diagnostic",
            ));
        }
        crate::derive::record_ledger_stale(&mut store, &gen::GenSettings::resolve(&project));
        store.save_for_test();
        gen::Ledger {
            medium: Some(gen::Medium {
                form: "Markdown page".into(),
                produced: "written".into(),
                toolchain: "text".into(),
                artifact: "src/site.md".into(),
            }),
            ..Default::default()
        }
        .save(&project.out)
        .unwrap();
        crate::vfs::write(
            crate::control::Control::path(&project.out),
            "compile: manual\ngenerate: manual\nreleased:\n  compile: {}\n  generate: 0\n",
        )
        .unwrap();
        fs.snapshot()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn workflow_request(
        files: crate::hosted::Snapshot,
        steps: Value,
        execution: &crate::hosted::Execution,
    ) -> (Value, crate::hosted::Snapshot) {
        let mut workspace = crate::hosted::Workspace::new(files).unwrap();
        let reply = workspace
            .request(
                "POST",
                "/api/jobs",
                &json!({"kind":"workflow", "setId":"ds_main", "steps":steps}).to_string(),
                execution,
            )
            .unwrap();
        (
            serde_json::from_slice(&reply.body).unwrap(),
            workspace.snapshot(),
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn compile_accepts_ratifications_and_gated_development_work() {
        let files = pending_workspace(29, 33, false);
        {
            let _fs = crate::vfs::Scope::enter(&files);
            let project = crate::document_sets::Workspace::load(Path::new("/project"))
                .unwrap()
                .resolve(None)
                .unwrap();
            let store = Store::load(&project.out);
            let control = crate::control::Control::load(&project, &project.out);
            let board = crate::board::Board::derive(&store, &project, &control);
            assert_eq!(
                board
                    .goals
                    .iter()
                    .filter(|goal| goal.kind == "ratify")
                    .count(),
                33
            );
            assert_eq!(
                board
                    .goals
                    .iter()
                    .filter(|goal| goal.kind == "bind" && board.gated.contains(&goal.id))
                    .count(),
                29
            );
            assert_eq!(
                board
                    .goals
                    .iter()
                    .filter(|goal| goal.kind == "generate" && board.gated.contains(&goal.id))
                    .count(),
                1
            );
            assert_eq!(board.counts().blocked, 63);
            assert!(
                board.verdict().converged(),
                "{:?} {:?}",
                board.verdict(),
                board
                    .goals
                    .iter()
                    .filter(|g| g.kind != "ratify" && !board.gated.contains(&g.id))
                    .map(|g| g.id.clone())
                    .collect::<Vec<_>>()
            );
            graph_ready(&project, &store).unwrap();
        }
        let (result, _) = workflow_request(
            files,
            json!([{"action":"compile"}]),
            &crate::hosted::Execution::default(),
        );
        assert_eq!(result["report"]["state"], "done", "{result}");
        let report = &result["report"]["steps"][0]["result"];
        assert!(
            report["verdict"].as_str().unwrap().starts_with("converged"),
            "{report}"
        );
        assert_eq!(report["sessions"], 0);
        assert_eq!(report["blocked"], 63);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn downstream_readiness_rejects_changed_documents_and_unfinished_graph_states() {
        use crate::model::*;
        let files = pending_workspace(1, 1, false);
        let _fs = crate::vfs::Scope::enter(&files);
        let project = crate::document_sets::Workspace::load(Path::new("/project"))
            .unwrap()
            .resolve(None)
            .unwrap();
        let mut store = Store::load(&project.out);
        graph_ready(&project, &store).unwrap();
        crate::vfs::write(project.root.join("docs/new.md"), [0xff]).unwrap();
        assert!(
            graph_ready(&project, &store)
                .unwrap_err()
                .starts_with("Documents changed."),
            "A new unreadable document cannot disappear from readiness"
        );
        store.sync_docs(&crate::reconcile::parse_all(&project).0);
        assert!(graph_ready(&project, &store)
            .unwrap_err()
            .starts_with("Documents changed."));
        crate::vfs::remove_file(project.root.join("docs/new.md")).unwrap();
        crate::vfs::write(project.root.join("docs/main.md"), [0xff]).unwrap();
        store.sync_docs(&crate::reconcile::parse_all(&project).0);
        assert!(
            graph_ready(&project, &store)
                .unwrap_err()
                .starts_with("Documents changed."),
            "Retaining the previous section tree does not make unreadable input current"
        );
        crate::vfs::write(
            project.root.join("docs/main.md"),
            "# Site\n\nSite shows item 1.\n",
        )
        .unwrap();
        graph_ready(&project, &store).unwrap();
        crate::vfs::write(
            project.root.join("docs/main.md"),
            "# Site\n\nThe documents changed.\n",
        )
        .unwrap();
        assert!(graph_ready(&project, &store)
            .unwrap_err()
            .starts_with("Documents changed."));
        crate::vfs::write(
            project.root.join("docs/main.md"),
            "# Site\n\nSite shows item 1.\nSite shows another item.\n",
        )
        .unwrap();
        store.sync_docs(&crate::reconcile::parse_all(&project).0);
        assert!(graph_ready(&project, &store)
            .unwrap_err()
            .contains("waiting on a release"));
        let mut board = crate::board::Board::derive(
            &store,
            &project,
            &crate::control::Control::load(&project, &project.out),
        );
        for state in [
            GoalState::Open,
            GoalState::Parked,
            GoalState::Failed {
                reason: "Needs repair.".into(),
            },
        ] {
            board.goals = vec![Goal {
                id: "g:review-entity:ent:site".into(),
                kind: "review-entity".into(),
                mandatory: true,
                state,
                ..Default::default()
            }];
            board.parked_reasons.insert(
                "g:review-entity:ent:site".into(),
                "reopened-after-resolve: same change twice".into(),
            );
            let error = graph_complete(&board).unwrap_err();
            assert!(error.contains("g:review-entity:ent:site"), "{error}");
            match &board.goals[0].state {
                GoalState::Open => assert!(error.contains("(open)"), "{error}"),
                GoalState::Parked => assert!(
                    error.contains("parked: reopened-after-resolve: same change twice"),
                    "{error}"
                ),
                GoalState::Failed { reason } => assert!(error.contains(reason), "{error}"),
                _ => unreachable!(),
            }
            assert!(
                !error.contains("Compile documents before continuing"),
                "{error}"
            );
        }
        let blocker = board.goals[0].clone();
        board.goals = (0..7)
            .map(|i| Goal {
                id: format!("g:review-entity:ent:site-{i}"),
                ..blocker.clone()
            })
            .collect();
        let error = graph_complete(&board).unwrap_err();
        assert!(error.contains("2 more"), "{error}");
        assert!(!error.contains("site-5"), "{error}");
        // Scope, optional work, ledger work, and human waits cannot inflate this error.
        for (i, goal) in board.goals.iter_mut().enumerate() {
            match i {
                0 => {
                    board.outside.insert(goal.id.clone());
                }
                1 => goal.mandatory = false,
                2 => goal.kind = "verify".into(),
                _ => {
                    goal.state = GoalState::Blocked {
                        on: "human: ratification".into(),
                    }
                }
            }
        }
        graph_complete(&board).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn generation_is_ready_before_binding_and_preserves_explicit_binding() {
        use crate::goals::GoalKind;
        let files = pending_workspace(1, 1, false);
        let _fs = crate::vfs::Scope::enter(&files);
        let project = crate::document_sets::Workspace::load(Path::new("/project"))
            .unwrap()
            .resolve(None)
            .unwrap();
        crate::control::release(&project, &project.out, Some("generate"));
        let store = Store::load(&project.out);
        let control = crate::control::Control::load(&project, &project.out);
        let board = crate::board::Board::derive(&store, &project, &control);
        let generation = board
            .goal("g:generate:ent:site")
            .expect("missing implementation is development work");
        let binding = board
            .goal("g:bind:req:site-1")
            .expect("explicit binding remains available");
        assert!(matches!(
            crate::goals::Generate.ready(generation, &board),
            crate::goals::Ready::Ready
        ));
        assert!(matches!(
            crate::goals::Bind.ready(binding, &board),
            crate::goals::Ready::Ready
        ));
        let ledger_goals: Vec<_> = board
            .batches
            .iter()
            .flat_map(|batch| batch.goals.iter())
            .filter_map(|id| board.goal(id))
            .filter(|goal| matches!(goal.kind.as_str(), "generate" | "bind"))
            .collect();
        assert_eq!(ledger_goals.first().unwrap().kind, "generate");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_fresh_workflow_writes_source_before_tests_without_a_binding_phase() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let mut files = pending_workspace(1, 1, false);
        let config = String::from_utf8(files[".jazyk/jazyk.toml"].clone())
            .unwrap()
            .replace(
                "docs = ['docs/**']",
                "docs = ['**', '!src/**', '!tests/**']",
            );
        files.insert(".jazyk/jazyk.toml".into(), config.into_bytes());
        files.insert(
            "logo.svg".into(),
            br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#.to_vec(),
        );
        files.insert(
            "metadata.json".into(),
            br#"{"name":"UTF-8 asset"}"#.to_vec(),
        );
        let (fact_hash, test_name, criteria_file) = {
            let _fs = crate::vfs::Scope::enter(&files);
            let project = crate::document_sets::Workspace::load(Path::new("/project"))
                .unwrap()
                .resolve(None)
                .unwrap();
            let store = Store::load(&project.out);
            assert!(gen::Ledger::load(&project.out).requirements.is_empty());
            let criterion = &store.graph.criteria["crit:site-1"];
            (gen::fact_hash(&store, "ent:site"), gen::crit_test_name("crit:site-1", &criterion.statement),
                format!("{}\nInspect src/site.md.\nPASS when Item 1 is visible.\nFAIL when Item 1 is absent.\n", gen::criteria_front_matter("req:site-1", Some("crit:site-1"), &criterion.statement)))
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let execution = crate::hosted::Execution {
            transport_async: Some(Arc::new(move |body| {
                let round = observed.fetch_add(1, Ordering::Relaxed);
                let request = serde_json::from_str(&body).unwrap();
                if let Some(reply) = crate::input::scripted_blob_reply(&request, None) {
                    return Box::pin(async move { Ok(reply.to_string()) });
                }
                assert_eq!(round, 2, "unexpected extra model request: {body}");
                let actions = [
                    ("begin_generation", json!({"entity":"ent:site"})),
                    (
                        "write_text_file",
                        json!({"path":"src/site.md","content":"# Site\n\nItem 1 is visible.\n"}),
                    ),
                    (
                        "write_text_file",
                        json!({"path":"tests/site.md","content":criteria_file}),
                    ),
                    (
                        "record_generation",
                        json!({"entity":"ent:site","factHash":fact_hash,"manifest":{
                            "files":["src/site.md"],"tests":[{"requirement":"req:site-1","criterion":"crit:site-1","kind":"llm","label":"Visible item","artifact":"tests/site.md","name":test_name,"run":"llm"}]
                        }}),
                    ),
                    (
                        "done",
                        json!({"summary":"Created source and its criterion check together."}),
                    ),
                ];
                let reply = json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":actions.iter().enumerate().map(|(index,(name,args))|
                json!({"id":format!("adaptive-{index}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()}}],"usage":{"prompt_tokens":100,"completion_tokens":20,"total_tokens":120}}).to_string();
                Box::pin(async move { Ok(reply) })
            })),
            ..Default::default()
        };
        let (result, files) = workflow_request(
            files,
            json!([{"action":"compile"},{"action":"generate"}]),
            &execution,
        );
        assert_eq!(result["report"]["state"], "done", "{result}");
        assert_eq!(result["report"]["steps"].as_array().unwrap().len(), 2);
        assert!(result["report"]["steps"][0]["result"]["verdict"]
            .as_str()
            .unwrap()
            .starts_with("converged"));
        assert_eq!(result["report"]["steps"][0]["result"]["sessions"], 2);
        assert_eq!(result["report"]["steps"][0]["result"]["warnings"], 0,
            "Opaque inputs compile with justified decisions rather than unsupported-format warnings");
        assert_eq!(result["report"]["steps"][1]["result"]["regenerated"], 1);
        assert_eq!(calls.load(Ordering::Relaxed), 3);
        assert_eq!(files["src/site.md"], b"# Site\n\nItem 1 is visible.\n");
        let _fs = crate::vfs::Scope::enter(&files);
        let ledger = gen::Ledger::load(Path::new("/project/.jazyk/ds_main"));
        assert!(ledger.requirements["req:site-1"]
            .tests
            .contains_key("crit:site-1"));
        assert_ne!(
            ledger.requirements["req:site-1"].verdict, "pass",
            "unexecuted criteria must not become passing evidence"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn incomplete_compile_keeps_its_report_and_stops_the_next_action() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let files = std::collections::BTreeMap::from([
            (".jazyk/jazyk.toml".into(), b"version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Main'\ndocs = ['docs/**']\nsource = ['src/**']\n".to_vec()),
            ("docs/main.md".into(), b"# Site\n\nSite shows an item.\n".to_vec()),
        ]);
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let execution = crate::hosted::Execution {
            transport_async: Some(Arc::new(move |_| {
                observed.fetch_add(1, Ordering::Relaxed);
                Box::pin(async {
                    Ok(json!({"choices":[{"message":{"role":"assistant","content":"The graph work remains unfinished."}}]}).to_string())
                })
            })),
            ..Default::default()
        };
        let (result, _) = workflow_request(
            files,
            json!([{"action":"compile"},{"action":"generate"}]),
            &execution,
        );
        assert_eq!(result["report"]["state"], "failed", "{result}");
        assert_eq!(result["report"]["steps"].as_array().unwrap().len(), 1);
        let report = &result["report"]["steps"][0]["result"];
        assert!(
            report["verdict"]
                .as_str()
                .unwrap()
                .starts_with("incomplete"),
            "{report}"
        );
        assert!(report["parked"].as_u64().unwrap() > 0);
        assert!(report["sessions"].as_u64().unwrap() > 0);
        assert!(report["error"]
            .as_str()
            .unwrap()
            .contains("Mandatory graph work remains"));
        let error = report["error"].as_str().unwrap();
        assert!(error.contains("g:"), "{error}");
        assert!(error.contains("parked"), "{error}");
        assert!(
            !error.contains("Compile documents before continuing"),
            "{error}"
        );
        assert!(calls.load(Ordering::Relaxed) > 0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_refused_compile_cannot_advance_a_ready_board_to_generation() {
        let files = pending_workspace(1, 1, false);
        let files = {
            let fs = crate::vfs::Scope::enter(&files);
            crate::control::claim(
                Path::new("/project/.jazyk/ds_main"),
                "external-batch",
                "another-worker",
            )
            .unwrap();
            fs.snapshot()
        };
        let (result, files) = workflow_request(
            files,
            json!([{"action":"compile"},{"action":"generate"}]),
            &crate::hosted::Execution::default(),
        );
        assert_eq!(result["report"]["state"], "failed", "{result}");
        assert_eq!(result["report"]["steps"].as_array().unwrap().len(), 1);
        let report = &result["report"]["steps"][0]["result"];
        assert_eq!(report["verdict"], "incomplete");
        assert_eq!(report["errors"], 1);
        assert!(report["error"]
            .as_str()
            .unwrap()
            .contains("could not start"));
        assert!(files
            .keys()
            .any(|path| path.contains("/leases/external-batch")));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_binding_transport_error_stops_before_the_next_target_and_retains_its_reason() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        };
        let mut workspace = crate::hosted::Workspace::new(pending_workspace(2, 2, false)).unwrap();
        let created = workspace
            .request("POST", "/api/chat/sessions", "{}", &Default::default())
            .unwrap();
        let chat: Value = serde_json::from_slice(&created.body).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let events = Arc::new(Mutex::new(Vec::new()));
        let recorded = events.clone();
        let execution = crate::hosted::Execution {
            transport_async: Some(Arc::new(move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Err("Local model request failed.".into()) })
            })),
            trace: Some(Arc::new(move |event| {
                recorded
                    .lock()
                    .unwrap()
                    .push(serde_json::to_value(event).unwrap())
            })),
            ..Default::default()
        };
        let request = json!({"kind":"workflow","setId":"ds_main","conversationId":chat["id"],"jobId":494,
            "steps":[{"action":"compile"},{"action":"bind"},{"action":"generate"}]});
        let reply = workspace
            .request("POST", "/api/jobs", &request.to_string(), &execution)
            .unwrap();
        let result: Value = serde_json::from_slice(&reply.body).unwrap();
        assert_eq!(reply.status, 422, "{result}");
        assert_eq!(result["report"]["steps"].as_array().unwrap().len(), 2);
        let binding = &result["report"]["steps"][1]["result"];
        assert_eq!(binding["bound"], 0);
        assert_eq!(binding["failures"], 1);
        assert!(
            binding["stopped"]
                .as_str()
                .unwrap()
                .contains("Local model request failed."),
            "{result}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let events = events.lock().unwrap();
        let sessions: Vec<_> = events
            .iter()
            .filter(|event| event["kind"] == "sessionStart")
            .collect();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["label"], "g:bind:req:site-1");
        drop(events);
        let mut restored = crate::hosted::Workspace::new(workspace.snapshot()).unwrap();
        let reply = restored
            .request(
                "GET",
                &format!("/api/chat/sessions/{}", chat["id"].as_str().unwrap()),
                "",
                &Default::default(),
            )
            .unwrap();
        let timeline: Value = serde_json::from_slice(&reply.body).unwrap();
        assert!(timeline["updates"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |row| row["update"]["sessionUpdate"] == "workflow" && row["update"]["jobId"] == 494
            ));
        let snapshot = restored.snapshot();
        let _fs = crate::vfs::Scope::enter(&snapshot);
        assert!(gen::Ledger::load(Path::new("/project/.jazyk/ds_main"))
            .requirements
            .is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_committed_binding_wins_over_a_later_model_error() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let files = pending_workspace(1, 1, false);
        let criteria = {
            let _fs = crate::vfs::Scope::enter(&files);
            let store = Store::load(Path::new("/project/.jazyk/ds_main"));
            gen::criteria_front_matter(
                "req:site-1",
                Some("crit:site-1"),
                &store.graph.criteria["crit:site-1"].statement,
            ) + "\nInspect the product. PASS when Item 1 is visible. FAIL when absent.\n"
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let execution = crate::hosted::Execution {
            transport_async: Some(Arc::new(move |_| {
                let round = observed.fetch_add(1, Ordering::SeqCst);
                let criteria = criteria.clone();
                Box::pin(async move {
                    if round > 0 {
                        return Err("Post-record model connection closed.".into());
                    }
                    let actions = [
                        ("begin_binding", json!({"requirement":"req:site-1"})),
                        (
                            "write_text_file",
                            json!({"path":"tests/site.md","content":criteria}),
                        ),
                        (
                            "record_binding",
                            json!({"requirement":"req:site-1","files":[],"verdict":"fail","tests":[{
                                "criterion":"crit:site-1","kind":"llm","artifact":"tests/site.md","label":"Visible item","name":"visible","run":"llm","verdict":"fail","evidence":"No implementation exists."
                            }]}),
                        ),
                    ];
                    Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":actions.iter().enumerate().map(|(index,(name,args))|
                        json!({"id":format!("record-{index}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()}}]}).to_string())
                })
            })),
            ..Default::default()
        };
        let (result, files) = workflow_request(files, json!([{"action":"bind"}]), &execution);
        assert_eq!(result["report"]["state"], "done", "{result}");
        assert_eq!(result["report"]["steps"][0]["result"]["bound"], 1);
        assert_eq!(result["report"]["steps"][0]["result"]["failures"], 0);
        assert!(result["report"]["steps"][0]["result"]["stopped"].is_null());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let _fs = crate::vfs::Scope::enter(&files);
        assert!(gen::Ledger::load(Path::new("/project/.jazyk/ds_main"))
            .requirements
            .contains_key("req:site-1"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn selected_binding_and_generation_write_a_deliverable_with_open_human_questions() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let files = pending_workspace(1, 1, true);
        let (fact_hash, test_name, criteria_file) = {
            let _fs = crate::vfs::Scope::enter(&files);
            let project = crate::document_sets::Workspace::load(Path::new("/project"))
                .unwrap()
                .resolve(None)
                .unwrap();
            let store = Store::load(&project.out);
            let settings = gen::GenSettings::resolve(&project);
            let binding = crate::bind::task(&store, "req:site-1", &settings).unwrap();
            assert_eq!(binding["openDiagnostics"][0]["id"], "diag:contradiction-1");
            let package = gen::task_package(&store, "ent:site", &settings).unwrap();
            assert!(package.to_string().contains("diag:contradiction-1"));
            let criterion = &store.graph.criteria["crit:site-1"];
            (gen::fact_hash(&store, "ent:site"), gen::crit_test_name("crit:site-1", &criterion.statement),
                format!("{}\nInspect src/site.md.\nPASS when Item 1 is visible.\nFAIL when Item 1 is absent.\n", gen::criteria_front_matter("req:site-1", Some("crit:site-1"), &criterion.statement)))
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let execution = crate::hosted::Execution {
            transport_async: Some(Arc::new(move |body| {
                let round = observed.fetch_add(1, Ordering::Relaxed);
                let actions = match round {
                    0 => vec![
                        ("begin_binding", json!({"requirement":"req:site-1"})),
                        (
                            "write_text_file",
                            json!({"path":"tests/site.md","content":criteria_file}),
                        ),
                        (
                            "record_binding",
                            json!({"requirement":"req:site-1","files":[],"verdict":"fail","tests":[{
                                "criterion":"crit:site-1","kind":"llm","label":"Visible item","artifact":"tests/site.md","name":test_name,"run":"llm","verdict":"fail","evidence":"No product exists yet."
                            }]}),
                        ),
                        (
                            "done",
                            json!({"summary":"Bound the visible-item criterion; the product is unimplemented."}),
                        ),
                    ],
                    1 => vec![
                        ("begin_generation", json!({"entity":"ent:site"})),
                        (
                            "write_text_file",
                            json!({"path":"src/site.md","content":"# Site\n\nItem 1 is visible.\n"}),
                        ),
                        (
                            "record_generation",
                            json!({"entity":"ent:site","factHash":fact_hash,"manifest":{
                                "files":["src/site.md"],"tests":[{"requirement":"req:site-1","criterion":"crit:site-1","kind":"llm","label":"Visible item","artifact":"tests/site.md","name":test_name,"run":"llm"}]
                            }}),
                        ),
                        (
                            "done",
                            json!({"summary":"Wrote and recorded the site page against the bound criterion."}),
                        ),
                    ],
                    _ => panic!("Unexpected model request {round}: {body}"),
                };
                let message = json!({"role":"assistant","content":null,"tool_calls":actions.iter().enumerate().map(|(index,(name,args))|
                json!({"id":format!("workflow-{round}-{index}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()});
                let reply = json!({"choices":[{"message":message}],"usage":{"prompt_tokens":100,"completion_tokens":20,"total_tokens":120}}).to_string();
                Box::pin(async move { Ok(reply) })
            })),
            ..Default::default()
        };
        let (result, files) = workflow_request(
            files,
            json!([{"action":"compile"},{"action":"bind"},{"action":"generate"}]),
            &execution,
        );
        assert_eq!(result["report"]["state"], "done", "{result}");
        assert_eq!(result["report"]["steps"].as_array().unwrap().len(), 3);
        assert_eq!(result["report"]["steps"][1]["result"]["bound"], 1);
        assert_eq!(result["report"]["steps"][2]["result"]["regenerated"], 1);
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert_eq!(files["src/site.md"], b"# Site\n\nItem 1 is visible.\n");
        let _fs = crate::vfs::Scope::enter(&files);
        let out = Path::new("/project/.jazyk/ds_main");
        let ledger = gen::Ledger::load(out);
        assert_eq!(
            ledger.contradicted["req:site-1"],
            vec!["diag:contradiction-1"]
        );
        let store = Store::load(out);
        assert_eq!(
            store.graph.diagnostics["diag:contradiction-1"].lifecycle,
            "open"
        );
        assert_eq!(
            store.graph.diagnostics["diag:ratification-pending-1"].lifecycle,
            "open"
        );
    }
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
        assert!(!successful(
            &json!({"error":"Graph work remains.","verdict":"incomplete","sessions":2})
        ));
        assert!(!successful(&json!({"failed":1})));
        assert!(!successful(&json!({"runnerFailed":1})));
        assert!(!successful(&json!({"build":{"ok":false}})));
        assert!(successful(&json!({"passed":3,"failed":0})));
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn reclassified_implementation_skips_testing_and_fails_before_the_next_action() {
        let files = {
            let seed = pending_workspace(1, 0, false);
            let fs = crate::vfs::Scope::enter(&seed);
            let project = crate::document_sets::Workspace::load(Path::new("/project"))
                .unwrap()
                .resolve(None)
                .unwrap();
            let store = Store::load(&project.out);
            let gs = gen::GenSettings::resolve(&project);
            crate::vfs::create_dir_all("/project/src").unwrap();
            crate::vfs::create_dir_all("/project/tests").unwrap();
            crate::vfs::write("/project/src/site.rs", "// Site implementation\n").unwrap();
            crate::vfs::write("/project/tests/site.rs", "fn site_test() {}\n").unwrap();
            crate::bind::record(&store, "req:site-1", &["src/site.rs".into()],
                &json!({"kind":"programmatic","artifact":"tests/site.rs","name":"site_test","run":"must-not-execute"}),
                &Value::Null, "pass", None, &gs).unwrap();
            crate::vfs::write("/project/.jazyk/jazyk.toml",
                "version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Main'\ndocs = ['docs/**', 'src/site.rs']\nroots = ['docs/main.md']\nsource = ['tests/**']\n").unwrap();
            fs.snapshot()
        };
        let before = files[".jazyk/ds_main/gen/ledger.yaml"].clone();
        let (result, files) = workflow_request(
            files,
            json!([{"action":"test"},{"action":"build"}]),
            &Default::default(),
        );
        assert_eq!(result["report"]["state"], "failed", "{result}");
        let steps = result["report"]["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 1, "{result}");
        assert_eq!(steps[0]["state"], "failed");
        assert_eq!(steps[0]["result"]["passed"], 0);
        assert!(steps[0]["result"]["skipped"][0]["reason"]
            .as_str()
            .unwrap()
            .starts_with("source-selection-changed:"));
        assert_eq!(files["src/site.rs"], b"// Site implementation\n");
        assert_eq!(files[".jazyk/ds_main/gen/ledger.yaml"], before);
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
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn restored_workflow_keeps_completed_results_and_skips_completed_task_definitions() {
        let _vfs = crate::vfs::Scope::enter(&std::collections::BTreeMap::new());
        let _scope = crate::session_recovery::Scope::enter(None);
        let project = Project::load(std::path::Path::new("/project"));
        let steps = vec![
            Step::Task {
                task_id: "old-task".into(),
            },
            Step::Build,
        ];
        let key =
            crate::session_recovery::command_key(&format!("workflow:{}", project.out.display()));
        let progress = Progress {
            version: 1,
            steps: steps.clone(),
            selected_tasks: vec![],
            next: 1,
            active: Some(1),
            results: vec![
                json!({"step":steps[0],"state":"done","result":{"runId":"original-run"}}),
            ],
            finished: None,
        };
        progress.save(&key).unwrap();
        let llm = crate::llm::Llm {
            api: "chat_completions".into(),
            reasoning_summary: None,
            base_url: String::new(),
            model: String::new(),
            api_key: String::new(),
            temperature: None,
            trace: None,
        };
        let trace = Trace::to_sink(
            crate::session::TraceLevel::Normal,
            std::sync::Arc::new(|_| {}),
            Default::default(),
        );
        let result = run(&project, &llm, &steps, &trace).unwrap();
        assert_eq!(result["state"], "done");
        assert_eq!(result["steps"][0]["result"]["runId"], "original-run");
        assert_eq!(result["steps"].as_array().unwrap().len(), 2);
        assert_eq!(run(&project, &llm, &steps, &trace).unwrap(), result);
        assert!(Progress::load(&key, &[Step::Build])
            .err()
            .unwrap()
            .starts_with("checkpoint-incompatible:"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn deployment_suspension_retains_the_active_workflow_action_without_failure() {
        let config =
            "version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Main'\ndocs = []\nsource = []\n";
        let mut workspace = crate::hosted::Workspace::new(
            [(".jazyk/jazyk.toml".into(), config.as_bytes().to_vec())].into(),
        )
        .unwrap();
        let saved = std::sync::Arc::new(std::sync::Mutex::new(None));
        let capture = saved.clone();
        let sequence = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let execution = crate::hosted::Execution {
            suspend: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            checkpoint: Some(std::sync::Arc::new(move |capsule, _| {
                *capture.lock().unwrap() = Some(capsule);
                let sequence = sequence.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                Box::pin(async move { Ok(sequence) })
            })),
            ..Default::default()
        };
        let request =
            json!({"kind":"workflow","setId":"ds_main","steps":[{"action":"build"}]}).to_string();
        let reply = workspace
            .request("POST", "/api/jobs", &request, &execution)
            .unwrap();
        let result: Value = serde_json::from_slice(&reply.body).unwrap();
        assert_eq!(result["report"]["state"], "suspended", "{result}");
        assert_eq!(result["report"]["steps"], json!([]));
        let resume = saved.lock().unwrap().clone().unwrap();
        let progress = resume["parent"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap();
        assert_eq!(progress["active"], 0);
        let reply = workspace
            .request(
                "POST",
                "/api/jobs",
                &request,
                &crate::hosted::Execution {
                    resume: Some(resume),
                    ..Default::default()
                },
            )
            .unwrap();
        let result: Value = serde_json::from_slice(&reply.body).unwrap();
        assert_eq!(result["report"]["state"], "done", "{result}");
        assert_eq!(result["report"]["steps"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn recovery_signals_are_nonterminal_workflow_outcomes() {
        for reason in [
            "suspended",
            "checkpoint-incompatible: old",
            "checkpoint-failed: storage",
            "execution-uncertain: task",
        ] {
            assert_eq!(
                interruption(&json!({"suspendReason":reason})),
                Some(reason.into())
            );
            assert_eq!(
                interruption(&json!({"stopped":reason})),
                Some(reason.into())
            );
        }
        assert!(interruption(&json!({"error":"ordinary model failure"})).is_none());
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
