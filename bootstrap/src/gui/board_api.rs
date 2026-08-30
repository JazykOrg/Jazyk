// The board and causality endpoints: the goal board as the reconciler derives it,
// the next session's prompt, the explanation of a goal or target, and the ripple
// DAG over the journal. Mirrors docs/frontends/gui.md#api (the board and causality).
use super::state::SharedState;
use crate::project::Project;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

// The board reply: every goal (with its readiness tier), the batches the scheduler
// would form, the counts, the verdict, and the CLI's summary line.
pub fn board_value(proj: &Project, out: &Path) -> Value {
    let board = crate::board::Board::compute(proj, out);
    let mut v = board.answer();
    if let Some(goals) = v["goals"].as_array_mut() {
        for g in goals {
            let tier = g["kind"].as_str().and_then(crate::goals::tier);
            g["tier"] = json!(tier);
        }
    }
    v["verdict"] = json!(board.verdict().to_string());
    v["summary"] = json!(board.summary_line());
    v
}

pub async fn board(State(st): State<SharedState>) -> Json<Value> {
    let proj = st.proj();
    let out = st.out.clone();
    Json(
        tokio::task::spawn_blocking(move || board_value(&proj, &out))
            .await
            .expect("board task panicked"),
    )
}

#[derive(Deserialize)]
pub struct PreviewQ {
    goal: Option<String>,
}

// The next session's prompt exactly as the model receives it, plus the batch's
// toolset and the executor it resolves to. With ?goal=, the batch that goal would
// join. Mirrors docs/frontends/gui.md#preview and docs/compiler/sessions.md#preview.
pub fn preview_value(proj: &Project, out: &Path, target: &str) -> Value {
    let mut store = crate::store::Store::load(out);
    let (parsed, _) = crate::reconcile::parse_all(proj);
    store.sync_docs(&parsed);
    let control = crate::control::Control::load(proj, out);
    let board = crate::board::Board::derive(&store, proj, &control);
    let target = target.trim();
    // ratify and answer goals have no session; preview prints what the human owes.
    if let Some(g) = board.goal(target) {
        if crate::goals::blocked_on_human(&g.kind) {
            let text = board
                .explain(&store, target)
                .unwrap_or_else(|| format!("{} is blocked on the human", target));
            return json!({
                "batch": Value::Null,
                "prompt": text,
                "humanBlocked": true,
                "note": "this goal has no session; a human resolves it",
            });
        }
    }
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
        let note = if target.is_empty() {
            "no ready batch; the board says why".to_string()
        } else {
            format!("no ready batch holds `{}`; the board says why", target)
        };
        return json!({ "batch": Value::Null, "prompt": Value::Null, "note": note });
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
    let prompt = crate::session::session_prompt(&store, &goals, &loaded, &skills, &pb);
    let mut kinds: Vec<&str> = Vec::new();
    for g in &goals {
        if !kinds.contains(&g.kind.as_str()) {
            kinds.push(&g.kind);
        }
    }
    let toolset = crate::tools::toolset_for_kinds(&kinds);
    let executor = resolve_executor_name(proj, &goals);
    let mut v = json!({
        "batch": board.batch_json(batch),
        "prompt": prompt,
        "toolset": toolset,
    });
    match executor {
        Ok(name) => v["executor"] = json!(name),
        Err(e) => v["executorError"] = json!(e),
    }
    v
}

// The agent profile the batch's first goal resolves to, by the executor ladder.
fn resolve_executor_name(proj: &Project, goals: &[crate::model::Goal]) -> Result<String, String> {
    let Some(g) = goals.first() else {
        return Err("empty batch".into());
    };
    let global_acp = crate::project::load_global_acp();
    let global_execs = crate::project::load_global_executors();
    crate::acp::config::resolve_executor(
        None,
        &g.kind,
        &g.class,
        &proj.acp,
        &proj.executors,
        &global_acp,
        &global_execs,
        |n| std::env::var(n).ok(),
    )
    .map(|a| a.name)
}

pub async fn preview(State(st): State<SharedState>, Query(p): Query<PreviewQ>) -> Json<Value> {
    let proj = st.proj();
    let out = st.out.clone();
    let target = p.goal.unwrap_or_default();
    Json(
        tokio::task::spawn_blocking(move || preview_value(&proj, &out, &target))
            .await
            .expect("preview task panicked"),
    )
}

#[derive(Deserialize)]
pub struct ExplainQ {
    target: String,
}

// A goal: its change, cause, readiness, blockers. A node or section: the cone of
// goals a change to it would open. The same rendering as `jazyk explain`.
pub async fn explain(State(st): State<SharedState>, Query(p): Query<ExplainQ>) -> Response {
    let proj = st.proj();
    let out = st.out.clone();
    let target = p.target.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut store = crate::store::Store::load(&out);
        let (parsed, _) = crate::reconcile::parse_all(&proj);
        store.sync_docs(&parsed);
        let control = crate::control::Control::load(&proj, &out);
        let board = crate::board::Board::derive(&store, &proj, &control);
        let goal = board.goal(&target).map(|g| {
            let mut v = board.goal_json(g);
            v["tier"] = json!(crate::goals::tier(&g.kind));
            v
        });
        board
            .explain(&store, &target)
            .map(|text| json!({ "target": target, "text": text, "goal": goal }))
    })
    .await
    .expect("explain task panicked");
    match result {
        Some(v) => Json(v).into_response(),
        None => super::api::err(
            StatusCode::NOT_FOUND,
            format!("`{}` names no goal and no known target", p.target),
        ),
    }
}

#[derive(Deserialize)]
pub struct RippleQ {
    generation: Option<u64>,
    target: Option<String>,
    #[serde(default)]
    back: bool,
}

// The causality DAG over the journal, forward from a generation or the last cascade
// that touched a target; `back=true` walks causes instead. A generation's DAG
// doubles as the whole-build report.
pub async fn ripple(State(st): State<SharedState>, Query(p): Query<RippleQ>) -> Response {
    let root = match (&p.generation, &p.target) {
        (Some(g), _) => format!("g{}", g),
        (None, Some(t)) if !t.trim().is_empty() => t.trim().to_string(),
        _ => {
            return super::api::err(
                StatusCode::BAD_REQUEST,
                "pass generation= or target= to root the DAG",
            )
        }
    };
    let out = st.out.clone();
    let back = p.back;
    let result = tokio::task::spawn_blocking(move || {
        let store = crate::store::Store::load(&out);
        crate::reconcile::ripple(&store, &root, back).map(|tree| {
            let text = crate::reconcile::render_ripple(&tree);
            json!({ "root": root, "back": back, "tree": tree, "text": text })
        })
    })
    .await
    .expect("ripple task panicked");
    match result {
        Some(v) => Json(v).into_response(),
        None => super::api::err(
            StatusCode::NOT_FOUND,
            "the journal holds no entry touching that root",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project(name: &str) -> (Project, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("jazyk-gui-board-{}-{}", name, std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(
            root.join("docs/a.md"),
            "# Shop\n\nThe shop stores orders. An order holds items.\n",
        )
        .unwrap();
        let mut proj = Project::default();
        proj.root = root.clone();
        proj.docs_glob = vec!["docs/**/*.md".into()];
        let out = root.join("jazyk-out");
        proj.out = out.clone();
        (proj, out)
    }

    // The board endpoint's shape: goals with readiness and tier, batches, counts,
    // verdict, and the summary line, derived from disk with no LLM call.
    #[test]
    fn board_value_shapes_goals_counts_and_verdict() {
        let (proj, out) = temp_project("shape");
        let v = board_value(&proj, &out);
        let goals = v["goals"].as_array().expect("goals array");
        assert!(!goals.is_empty(), "an unprocessed section derives a goal");
        let g = goals
            .iter()
            .find(|g| g["kind"] == "reconcile-section")
            .expect("a reconcile-section goal");
        for key in [
            "id", "kind", "target", "class", "state", "ready", "gated", "tier",
        ] {
            assert!(!g[key].is_null() || key == "tier", "goal carries {}", key);
        }
        assert_eq!(g["tier"], json!(1));
        assert!(v["counts"]["by_kind"].is_object());
        assert!(v["verdict"].is_string());
        assert!(v["summary"].as_str().unwrap().starts_with("compile:"));
        assert!(v["batches"].is_array());
        std::fs::remove_dir_all(&proj.root).ok();
    }

    // A preview names the batch, assembles the prompt, and lists the toolset,
    // without spending anything.
    #[test]
    fn preview_value_assembles_a_prompt_for_the_first_batch() {
        let (proj, out) = temp_project("preview");
        // Auto mode so nothing is gated behind a release.
        let mut control = crate::control::Control::load(&proj, &out);
        control.compile = "auto".into();
        control.save(&out);
        let v = preview_value(&proj, &out, "");
        assert!(v["batch"]["id"]
            .as_str()
            .unwrap_or_default()
            .starts_with('b'));
        let prompt = v["prompt"].as_str().expect("a prompt");
        assert!(prompt.contains("## Goals"), "the goals block is present");
        assert!(prompt.contains("## Loaded"), "the loaded block is present");
        let toolset: Vec<&str> = v["toolset"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t.as_str())
            .collect();
        assert!(toolset.contains(&"done"));
        assert!(toolset.contains(&"load"));
        std::fs::remove_dir_all(&proj.root).ok();
    }
}
