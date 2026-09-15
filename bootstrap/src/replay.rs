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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/benchmark/replays")
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
        for block in blocks {
            let v: Value = serde_norway::from_str(&block)
                .map_err(|e| format!("{}: bad yaml: {}", f.display(), e))?;
            let case = benchmark::case_from_value(&v);
            if case.steps.is_empty() {
                return Err(format!("{}: `{}` has no steps; a replay scripts its calls", f.display(), case.name));
            }
            cases.push(case);
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
            problems.push(format!("expected rule `{}`, got a reply: {}", r, clip(&text)));
        } else if &rule != r {
            problems.push(format!("expected rule `{}`, got `{}`: {}", r, rule, clip(&text)));
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
        (true, clip(&text))
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
        benchmark::sandbox(case, &tmp)
    };
    let dirty: Vec<String> = match case.task_type.as_str() {
        "reconcile-doc" => store
            .docs
            .get(&case.target)
            .map(|r| r.sections.keys().cloned().collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    };
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
    let benchmark::CaseSession { mut session, .. } =
        benchmark::case_session(store.clone(), &item, &case.lint, &gs, derived.as_ref());
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
        let commit = session.commit(0, 0);
        let ops = std::mem::take(&mut session.staged);
        if derived.is_some() {
            store.apply(ops, &commit);
        } else {
            store.apply(ops, &item.commit(0, 0));
        }
    }
    for (kind, arg) in &case.checks {
        report
            .checks
            .push((kind.clone(), benchmark::eval_check(kind, arg, &store, staged)));
    }
    crate::vfs::remove_dir_all(&tmp).ok();
    report
}

pub fn print_report(r: &ReplayReport) {
    println!("replay {}: {}", r.name, if r.passed() { "ok" } else { "FAIL" });
    if let Some(e) = &r.fixture_error {
        println!("  fixture: {}", e);
    }
    for (i, s) in r.steps.iter().enumerate() {
        println!(
            "  step {} {}: {}{}",
            i + 1,
            s.call,
            if s.ok { "ok" } else { "FAIL " },
            if s.ok { String::new() } else { s.detail.clone() }
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
        let e = if v["event"].is_object() { &v["event"] } else { &v };
        if e["label"].as_str() != Some(label) {
            continue;
        }
        match e["kind"].as_str() {
            Some("sessionStart") => {
                if goals.is_empty() {
                    goals = e["goals"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|g| g.as_str().map(String::from)).collect())
                        .unwrap_or_default();
                }
            }
            Some("toolCall") => {
                let raw = e["full"].as_str().or(e["summary"].as_str()).unwrap_or("{}");
                let args: Value = serde_json::from_str(raw).unwrap_or_else(|_| json!({"_unparsed": raw}));
                calls.push((e["name"].as_str().unwrap_or("").to_string(), args));
            }
            Some("toolResult") => {
                let raw = e["full"].as_str().or(e["summary"].as_str()).unwrap_or("");
                let mut val: Value = serde_json::from_str(raw).unwrap_or(Value::String(raw.to_string()));
                if let Some(s) = val.as_str() {
                    if let Ok(inner) = serde_json::from_str::<Value>(s) {
                        val = inner;
                    }
                }
                results.push(val);
            }
            _ => {}
        }
    }
    if calls.is_empty() {
        return Err(format!("no tool calls under label `{}` in {}", label, trace.display()));
    }
    let goal_id = goals.first().cloned().unwrap_or_else(|| label.to_string());
    let (kind, target) = goal_id
        .strip_prefix("g:")
        .and_then(|rest| rest.split_once(':'))
        .map(|(k, t)| (k.to_string(), t.to_string()))
        .unwrap_or((String::new(), String::new()));
    // The project: the trace lives under <root>/jazyk-out/trace.
    let root = trace
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut docs = serde_json::Map::new();
    let mut files: Vec<PathBuf> = Vec::new();
    collect_md(&root, &root, &mut files);
    files.sort();
    for f in files {
        if let Ok(t) = std::fs::read_to_string(&f) {
            let rel = f.strip_prefix(&root).unwrap_or(&f).to_string_lossy().to_string();
            docs.insert(rel, Value::String(t));
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
    let case = json!({
        "name": format!("{}-{}", trace.file_stem().and_then(|s| s.to_str()).unwrap_or("trace"), label.replace(':', "-")),
        "description": format!("Extracted from {} session {}; trim the fixture, keep the steps that matter, correct the expectations", trace.display(), label),
        "goal": {"kind": kind, "target": target},
        "given": {"docs": docs},
        "steps": steps,
        "assert": [],
    });
    let yaml = serde_norway::to_string(&case).map_err(|e| e.to_string())?;
    Ok(format!(
        "# Replay {}\n\nExtracted from `{}` session `{}` ({} call(s), goals: {}). The graph the session assumed is not seeded: add the nodes it needs under `given.graph`.\n\n```yaml\n{}```\n",
        label,
        trace.display(),
        label,
        calls.len(),
        goals.join(", "),
        yaml
    ))
}

fn collect_md(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if matches!(name.as_str(), "jazyk-out" | "jazyk-out.bak" | "node_modules" | "target") || name.starts_with('.') {
                continue;
            }
            collect_md(root, &p, out);
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
