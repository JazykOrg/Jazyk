// Replays: a case whose tool calls are scripted, run against the harness without a
// model, so a fix is verified against the recorded scenario that exposed it.
// Mirrors docs/benchmark/replays.md.

use crate::benchmark::{self, Case, Step};
use crate::model::WorkItem;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct StepOutcome {
    pub call: String,
    pub ok: bool,
    pub detail: String,
}

pub struct ReplayReport {
    pub name: String,
    pub fixture_error: Option<String>,
    pub steps: Vec<StepOutcome>,
    // (check kind, failure), None when the check held.
    pub checks: Vec<(String, Option<String>)>,
}

impl ReplayReport {
    pub fn passed(&self) -> bool {
        self.fixture_error.is_none()
            && self.steps.iter().all(|s| s.ok)
            && self.checks.iter().all(|(_, f)| f.is_none())
    }
}

// The replays under docs/benchmark/replays, the default set.
pub fn default_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(file!())
        .parent()
        .expect("replay source has a parent directory")
        .join("../../docs/benchmark/replays")
}

// Every replay in the named files and directories, in path order.
pub fn parse_paths(paths: &[PathBuf]) -> Result<Vec<Case>, String> {
    let mut files: Vec<PathBuf> = Vec::new();
    for p in paths {
        if p.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(p)
                .map_err(|e| format!("cannot read {}: {}", p.display(), e))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|f| {
                    matches!(
                        f.extension().and_then(|x| x.to_str()),
                        Some("md") | Some("yaml") | Some("yml")
                    )
                })
                .collect();
            entries.sort();
            files.extend(entries);
        } else {
            files.push(p.clone());
        }
    }
    let mut cases = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f)
            .map_err(|e| format!("cannot read {}: {}", f.display(), e))?;
        let blocks = if f.extension().and_then(|x| x.to_str()) == Some("md") {
            benchmark::yaml_blocks(&text)
        } else {
            vec![text]
        };
        let before = cases.len();
        for block in blocks {
            let v: Value = serde_norway::from_str(&block)
                .map_err(|e| format!("{}: bad yaml: {}", f.display(), e))?;
            let case = benchmark::case_from_value(&v);
            if case.steps.is_empty() {
                return Err(format!(
                    "{}: `{}` has no steps; a replay scripts its calls",
                    f.display(),
                    case.name
                ));
            }
            cases.push(case);
        }
        // A file that grades nothing is an error, never a silent pass
        // (docs/benchmark/replays.md#running-replays).
        if cases.len() == before {
            return Err(format!(
                "{}: no replay found; a replay is a fenced ```yaml block (an unclosed fence reads as none)",
                f.display()
            ));
        }
    }
    Ok(cases)
}

// The reply text a step's expectation reads: the error's value, or the reply JSON.
fn matches(pattern: &str, text: &str) -> Result<bool, String> {
    regex::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map(|r| r.is_match(text))
        .map_err(|e| format!("bad pattern `{}`: {}", pattern, e))
}

fn judge(step: &Step, reply: &Result<Value, crate::tools::ToolError>) -> (bool, String) {
    let (is_err, rule, text) = match reply {
        Ok(v) => (false, String::new(), v.to_string()),
        Err(e) => (true, e.rule.clone(), e.to_value().to_string()),
    };
    let mut problems: Vec<String> = Vec::new();
    let want_ok = step.ok.unwrap_or(step.rule.is_none());
    if let Some(r) = &step.rule {
        if !is_err {
            problems.push(format!(
                "expected rule `{}`, got a reply: {}",
                r,
                clip(&text)
            ));
        } else if &rule != r {
            problems.push(format!(
                "expected rule `{}`, got `{}`: {}",
                r,
                rule,
                clip(&text)
            ));
        }
    } else if want_ok && is_err {
        problems.push(format!("expected ok, got `{}`: {}", rule, clip(&text)));
    } else if !want_ok && !is_err {
        problems.push(format!("expected an error, got a reply: {}", clip(&text)));
    }
    if let Some(p) = &step.contains {
        match matches(p, &text) {
            Ok(true) => {}
            Ok(false) => problems.push(format!("reply does not match `{}`: {}", p, clip(&text))),
            Err(e) => problems.push(e),
        }
    }
    if let Some(p) = &step.absent {
        match matches(p, &text) {
            Ok(false) => {}
            Ok(true) => problems.push(format!("reply matches `{}`: {}", p, clip(&text))),
            Err(e) => problems.push(e),
        }
    }
    if problems.is_empty() {
        // The line says what happened, not only that the expectation held
        // (docs/benchmark/replays.md#running-replays).
        let what = if is_err {
            format!("refused: {}", rule)
        } else {
            String::new()
        };
        (true, what)
    } else {
        (false, problems.join("; "))
    }
}

fn clip(s: &str) -> String {
    crate::llm::truncate(&s.replace('\n', " "), 200)
}

// Run one replay in a fresh sandbox. Mirrors docs/benchmark/replays.md#running-replays.
pub fn run_case(case: &Case) -> ReplayReport {
    let tmp = std::env::temp_dir().join(format!(
        "jazyk-replay-{}-{}",
        case.name.replace(|c: char| !c.is_ascii_alphanumeric(), "-"),
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&tmp).ok();
    let mut report = ReplayReport {
        name: case.name.clone(),
        fixture_error: None,
        steps: Vec::new(),
        checks: Vec::new(),
    };
    // The fixture seeds as a case's does: a derived goal through one commit, an
    // assembled one straight into the graph (docs/benchmark/cases.md#derived-goals).
    let mut derived: Option<crate::model::Goal> = None;
    let mut store = if case.derives_goal() {
        match benchmark::seed_derived(case, &tmp) {
            Ok((s, g)) => {
                derived = Some(g);
                s
            }
            Err(e) => {
                report.fixture_error = Some(e);
                return report;
            }
        }
    } else {
        if let Err(error) = benchmark::prepare_runtime_fixture(case, &tmp) {
            report.fixture_error = Some(error);
            return report;
        }
        benchmark::sandbox(case, &tmp)
    };
    let dirty = benchmark::case_dirty_sections(&store, case);
    let item = match &derived {
        Some(g) => WorkItem::from_goal(g),
        None => WorkItem {
            task: case.task_type.clone(),
            target: case.target.clone(),
            dirty_sections: dirty,
            stale_anchors: Vec::new(),
            proposals: Vec::new(),
        },
    };
    let gs = crate::gen::GenSettings {
        configured: true,
        deliverable: tmp.join("deliverable"),
        worker: "agentic".into(),
        code: Vec::new(),
    };
    crate::vfs::create_dir_all(&gs.deliverable).ok();
    for (path, content) in &case.deliverable {
        let p = gs.deliverable.join(path);
        if let Some(d) = p.parent() {
            crate::vfs::create_dir_all(d).ok();
        }
        crate::vfs::write(&p, content).ok();
    }
    // The batch's other goals derive from the same seeded board
    // (docs/benchmark/replays.md#replay-format).
    let mut extra: Vec<crate::model::Goal> = Vec::new();
    if !case.extra_goals.is_empty() {
        let proj = crate::project::Project {
            root: tmp.clone(),
            out: store.out.clone(),
            gen_deliverable: Some("deliverable".into()),
            ..Default::default()
        };
        let control = crate::control::Control::load(&proj, &proj.out);
        let board = crate::board::Board::derive(&store, &proj, &control);
        for (kind, target) in &case.extra_goals {
            // An assembled kind builds its goal from kind and target, as the primary
            // goal of such a case does (docs/benchmark/cases.md#derived-goals).
            if let Some(task) = benchmark::assembled_task(kind) {
                let dirty: Vec<String> = if task == "reconcile-doc" {
                    store
                        .docs
                        .get(target)
                        .map(|r| r.sections.keys().cloned().collect())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                extra.push(
                    WorkItem {
                        task: task.to_string(),
                        target: target.clone(),
                        dirty_sections: dirty,
                        stale_anchors: Vec::new(),
                        proposals: Vec::new(),
                    }
                    .to_goal(crate::model::GoalState::Open),
                );
                continue;
            }
            let id = crate::goals::goal_id(kind, target);
            match board.goals.iter().find(|g| g.id == id) {
                Some(g) => extra.push(g.clone()),
                None => {
                    let same: Vec<&str> = board
                        .goals
                        .iter()
                        .filter(|g| &g.kind == kind)
                        .map(|g| g.id.as_str())
                        .collect();
                    report.fixture_error = Some(format!(
                        "the fixture derives no {}; {} goals derived: {}",
                        id,
                        kind,
                        if same.is_empty() {
                            "(none)".to_string()
                        } else {
                            same.join(", ")
                        }
                    ));
                    return report;
                }
            }
        }
    }
    let benchmark::CaseSession { mut session, .. } = benchmark::case_session_batch(
        store.clone(),
        &item,
        &case.lint,
        &gs,
        derived.as_ref(),
        &extra,
    );
    for step in &case.steps {
        let reply = session.dispatch(&step.call, &step.args);
        let (ok, detail) = judge(step, &reply);
        report.steps.push(StepOutcome {
            call: step.call.clone(),
            ok,
            detail,
        });
    }
    // A done step commits as the model's would; without one the checks read the
    // seeded fixture (docs/benchmark/replays.md#replay-format).
    let staged = session.staged.len();
    if session.done.is_some() && staged > 0 {
        let commit = session.commit(0, None);
        let ops = std::mem::take(&mut session.staged);
        if derived.is_some() {
            store.apply(ops, &commit);
        } else {
            store.apply(ops, &item.commit(0, None));
        }
    }
    for (kind, arg) in &case.checks {
        report.checks.push((
            kind.clone(),
            benchmark::eval_check(kind, arg, &store, staged),
        ));
    }
    crate::vfs::remove_dir_all(&tmp).ok();
    report
}

pub fn print_report(r: &ReplayReport) {
    println!(
        "replay {}: {}",
        r.name,
        if r.passed() { "ok" } else { "FAIL" }
    );
    if let Some(e) = &r.fixture_error {
        println!("  fixture: {}", e);
    }
    for (i, s) in r.steps.iter().enumerate() {
        println!(
            "  step {} {}: {}{}",
            i + 1,
            s.call,
            if s.ok { "ok" } else { "FAIL " },
            match (s.ok, s.detail.is_empty()) {
                (true, true) => String::new(),
                (true, false) => format!(" ({})", s.detail),
                _ => s.detail.clone(),
            }
        );
    }
    for (k, f) in &r.checks {
        match f {
            None => println!("  check {}: ok", k),
            Some(m) => println!("  check {}: FAIL {}", k, m),
        }
    }
}

// `jazyk replay [file|dir...]` and `jazyk replay extract <trace> <label>`.
// Mirrors docs/frontends/cli.md#jazyk-replay.
pub fn run_cli(args: &[String]) -> i32 {
    if args.first().map(String::as_str) == Some("extract") {
        let (Some(trace), Some(label)) = (args.get(1), args.get(2)) else {
            eprintln!("usage: jazyk replay extract <trace.jsonl> <label>");
            return 2;
        };
        return match extract(Path::new(trace), label) {
            Ok(text) => {
                print!("{}", text);
                0
            }
            Err(e) => {
                eprintln!("jazyk: {}", e);
                2
            }
        };
    }
    let paths: Vec<PathBuf> = if args.is_empty() {
        vec![default_dir()]
    } else {
        args.iter().map(PathBuf::from).collect()
    };
    let cases = match parse_paths(&paths) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("jazyk: {}", e);
            return 2;
        }
    };
    let mut failed = 0usize;
    for case in &cases {
        let r = run_case(case);
        print_report(&r);
        if !r.passed() {
            failed += 1;
        }
    }
    println!("{} replay(s), {} failed", cases.len(), failed);
    if failed > 0 {
        1
    } else {
        0
    }
}

// One session of a trace as a replay file: the goal, the project's documents, the
// calls with their recorded outcomes. Mirrors docs/benchmark/replays.md#where-replays-come-from.
pub fn extract(trace: &Path, label: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(trace)
        .map_err(|e| format!("cannot read {}: {}", trace.display(), e))?;
    let mut goals: Vec<String> = Vec::new();
    let mut calls: Vec<(String, Value)> = Vec::new();
    let mut results: Vec<Value> = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let e = if v["event"].is_object() {
            &v["event"]
        } else {
            &v
        };
        if e["label"].as_str() != Some(label) {
            continue;
        }
        match e["kind"].as_str() {
            Some("sessionStart") => {
                if goals.is_empty() {
                    goals = e["goals"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|g| g.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                }
            }
            Some("toolCall") => {
                let raw = e["full"].as_str().or(e["summary"].as_str()).unwrap_or("{}");
                let args: Value =
                    serde_json::from_str(raw).unwrap_or_else(|_| json!({"_unparsed": raw}));
                calls.push((e["name"].as_str().unwrap_or("").to_string(), args));
            }
            Some("toolResult") => {
                let raw = e["full"].as_str().or(e["summary"].as_str()).unwrap_or("");
                let mut val: Value =
                    serde_json::from_str(raw).unwrap_or(Value::String(raw.to_string()));
                if let Some(s) = val.as_str() {
                    if let Ok(inner) = serde_json::from_str::<Value>(s) {
                        val = inner;
                    }
                }
                results.push(val);
            }
            // A build trace records a refusal as its own event in the call's place.
            Some("toolError") => {
                // The message carries the reply the model saw; its rule wins over the
                // event's field, which some writers fill with the tool name.
                let msg = e["message"].as_str().unwrap_or("");
                let rule = RULE_IN_MESSAGE
                    .captures(msg)
                    .map(|c| c[1].to_string())
                    .or_else(|| {
                        e["rule"]
                            .as_str()
                            .filter(|r| !r.is_empty())
                            .map(String::from)
                    })
                    .unwrap_or_else(|| "?".to_string());
                results.push(json!({"error": {"rule": rule, "message": msg}}));
            }
            _ => {}
        }
    }
    if calls.is_empty() {
        return Err(format!(
            "no tool calls under label `{}` in {}",
            label,
            trace.display()
        ));
    }
    let goal_id = goals.first().cloned().unwrap_or_else(|| label.to_string());
    let (kind, target) = goal_id
        .strip_prefix("g:")
        .and_then(|rest| rest.split_once(':'))
        .map(|(k, t)| (k.to_string(), t.to_string()))
        .unwrap_or((String::new(), String::new()));
    // The project: the trace lives under <root>/.jazyk/trace.
    let root = trace
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut docs = serde_json::Map::new();
    let mut files: Vec<PathBuf> = Vec::new();
    collect_md(&root, &mut files);
    files.sort();
    for f in files {
        if let Ok(t) = std::fs::read_to_string(&f) {
            let rel = f
                .strip_prefix(&root)
                .unwrap_or(&f)
                .to_string_lossy()
                .to_string();
            docs.insert(rel, Value::String(t));
        }
    }
    // The graph the session saw: the batch's snapshot when the run left one, else the
    // current graph, trimmed to the nodes the goal and the calls name
    // (docs/benchmark/replays.md#where-replays-come-from).
    let snap = root
        .join(".jazyk")
        .join("snapshots")
        .join(label.replace(['/', ':'], "_"))
        .join("graph");
    let (graph_dir, seeded_from) = if snap.is_dir() {
        (snap, "the batch's snapshot")
    } else {
        (
            root.join(".jazyk").join("graph"),
            "the project's current graph (no snapshot for this batch)",
        )
    };
    // One file per node (docs/compiler/graph.md#node-files).
    let read_kind = |kind: &str| crate::store::read_node_values(&graph_dir.join(kind));
    let ents = read_kind("entities");
    let reqs = read_kind("requirements");
    let views = read_kind("views");
    let mut wanted: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let id_re = regex::Regex::new(r"(ent|req|view|crit):[A-Za-z0-9_.-]+").unwrap();
    for g in &goals {
        for m in id_re.find_iter(g) {
            wanted.insert(m.as_str().to_string());
        }
    }
    for (_, args) in &calls {
        for m in id_re.find_iter(&args.to_string()) {
            wanted.insert(m.as_str().to_string());
        }
    }
    // A goal's entity brings the requirements that reference it (a review or a
    // criteria goal derives from them); requirements bring their entities; entities
    // bring their parent chain.
    for g in goals.clone() {
        for m in id_re.find_iter(&g) {
            let id = m.as_str();
            if id.starts_with("ent:") {
                for (rid, r) in &reqs {
                    if r["entities"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|e| e.as_str() == Some(id))
                    {
                        wanted.insert(rid.clone());
                    }
                }
            }
        }
    }
    let mut grow = true;
    while grow {
        grow = false;
        let snapshot: Vec<String> = wanted.iter().cloned().collect();
        for id in snapshot {
            if let Some(r) = reqs.get(&id) {
                for e in r["entities"].as_array().into_iter().flatten() {
                    if let Some(s) = e.as_str() {
                        grow |= wanted.insert(s.to_string());
                    }
                }
            }
            if let Some(e) = ents.get(&id) {
                if let Some(p) = e["parent"].as_str() {
                    grow |= wanted.insert(p.to_string());
                }
            }
        }
    }
    let sec_ref = |v: &Value| -> Value {
        json!({"section": format!("{}#{}", v["doc"].as_str().unwrap_or(""), v["section"].as_str().unwrap_or("")), "quote": v["quote"]})
    };
    let mut g_ents = serde_json::Map::new();
    let mut g_reqs = serde_json::Map::new();
    let mut g_views = serde_json::Map::new();
    for id in &wanted {
        if let Some(e) = ents.get(id) {
            if e["deleted"].as_bool() == Some(true) {
                continue;
            }
            let mut o = serde_json::Map::new();
            o.insert("name".into(), e["name"].clone());
            for k in [
                "aliases",
                "definition",
                "stereotype",
                "parent",
                "scope",
                "provenance",
            ] {
                if !e[k].is_null() && e[k] != json!("") && e[k] != json!([]) {
                    o.insert(k.into(), e[k].clone());
                }
            }
            o.insert(
                "mentions".into(),
                json!(e["mentions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(&sec_ref)
                    .collect::<Vec<_>>()),
            );
            g_ents.insert(id.clone(), Value::Object(o));
        } else if let Some(r) = reqs.get(id) {
            if r["deleted"].as_bool() == Some(true) {
                continue;
            }
            let mut o = serde_json::Map::new();
            o.insert("statement".into(), r["statement"].clone());
            o.insert("entities".into(), r["entities"].clone());
            if r["source"].is_object() {
                o.insert("source".into(), sec_ref(&r["source"]));
            }
            for k in ["edges", "transition", "facets"] {
                if !r[k].is_null() && r[k] != json!([]) {
                    o.insert(k.into(), r[k].clone());
                }
            }
            g_reqs.insert(id.clone(), Value::Object(o));
        } else if let Some(v) = views.get(id) {
            g_views.insert(id.clone(), v.clone());
        }
    }
    let mut steps: Vec<Value> = Vec::new();
    for (i, (name, args)) in calls.iter().enumerate() {
        if matches!(name.as_str(), "begin_goals" | "abandon_goals") {
            continue;
        }
        let mut step = json!({"call": name, "args": args});
        if let Some(r) = results.get(i) {
            if let Some(rule) = r["error"]["rule"].as_str() {
                step["expect"] = json!({"rule": rule});
            }
        }
        steps.push(step);
    }
    let extra_goals: Vec<Value> = goals
        .iter()
        .skip(1)
        .filter_map(|g| {
            let rest = g.strip_prefix("g:")?;
            let (k, t) = rest.split_once(':')?;
            Some(json!({"kind": k, "target": t}))
        })
        .collect();
    let case = json!({
        "name": format!("{}-{}", trace.file_stem().and_then(|s| s.to_str()).unwrap_or("trace"), label.replace(':', "-")),
        "goals": extra_goals,
        "description": format!("Extracted from {} session {}; trim the fixture, keep the steps that matter, correct the expectations", trace.display(), label),
        "goal": {"kind": kind, "target": target},
        "given": {"docs": docs, "graph": {"entities": g_ents, "requirements": g_reqs, "views": g_views}},
        "steps": steps,
        "assert": [],
    });
    let yaml = serde_norway::to_string(&case).map_err(|e| e.to_string())?;
    Ok(format!(
        "# Replay {}\n\nExtracted from `{}` session `{}` ({} call(s), goals: {}). The graph is seeded from {}: {} entities, {} requirements, {} views the goal and the calls name. Trim what the scenario does not need.\n\n```yaml\n{}```\n",
        label,
        trace.display(),
        label,
        calls.len(),
        goals.join(", "),
        seeded_from,
        g_ents.len(),
        g_reqs.len(),
        g_views.len(),
        yaml
    ))
}

// The `rule` a refusal message carries, quoted or JSON-escaped.
static RULE_IN_MESSAGE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r#"\\?"rule\\?":\s*\\?"([a-z-]+)"#).expect("a literal pattern")
});

fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if matches!(
                name.as_str(),
                ".jazyk" | ".jazyk.bak" | "node_modules" | "target"
            ) || name.starts_with('.')
            {
                continue;
            }
            collect_md(&p, out);
        } else if name.ends_with(".md") && name != "EXPECTED.md" {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every replay under docs/benchmark/replays holds: a harness change that breaks a
    // recorded scenario fails the build (docs/benchmark/replays.md#running-replays).
    #[test]
    fn replays_hold() {
        let cases = parse_paths(&[default_dir()]).expect("replays parse");
        assert!(!cases.is_empty(), "no replays found");
        let mut failures = Vec::new();
        for c in &cases {
            let r = run_case(c);
            if !r.passed() {
                let mut why = r.fixture_error.clone().unwrap_or_default();
                for s in r.steps.iter().filter(|s| !s.ok) {
                    why.push_str(&format!(" [{}: {}]", s.call, s.detail));
                }
                for (k, f) in r.checks.iter() {
                    if let Some(m) = f {
                        why.push_str(&format!(" [check {}: {}]", k, m));
                    }
                }
                failures.push(format!("{}:{}", r.name, why));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
