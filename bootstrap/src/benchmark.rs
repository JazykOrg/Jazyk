// The benchmark frontend: decides whether a model is capable of driving compilation
// turns. Runs every case under both codecs in a sandbox store, grades with
// deterministic checks, no LLM judge. Mirrors docs/benchmark/benchmark.md.
//
// The case definitions ARE the documentation: the fenced yaml blocks in
// docs/benchmark/cases/*.md are embedded at compile time, one source of truth.
use crate::llm::{self, Llm};
use crate::model::*;
use crate::project::{Limits, Linting};
use crate::store::Store;
use crate::turn::{Trace, TraceLevel};
use serde_json::Value;
use std::collections::BTreeMap;

const CASE_FILES: [&str; 15] = [
    include_str!("../../docs/benchmark/cases/turn-extract.md"),
    include_str!("../../docs/benchmark/cases/turn-navigation.md"),
    include_str!("../../docs/benchmark/cases/turn-declarative.md"),
    include_str!("../../docs/benchmark/cases/turn-density.md"),
    include_str!("../../docs/benchmark/cases/turn-edges.md"),
    include_str!("../../docs/benchmark/cases/turn-reuse.md"),
    include_str!("../../docs/benchmark/cases/turn-converge.md"),
    include_str!("../../docs/benchmark/cases/turn-repair.md"),
    include_str!("../../docs/benchmark/cases/turn-review.md"),
    include_str!("../../docs/benchmark/cases/turn-review-duplicate.md"),
    include_str!("../../docs/benchmark/cases/turn-review-lookalike.md"),
    include_str!("../../docs/benchmark/cases/turn-review-lint.md"),
    include_str!("../../docs/benchmark/cases/turn-steps.md"),
    include_str!("../../docs/benchmark/cases/gen-basic.md"),
    include_str!("../../docs/benchmark/cases/verify-judge.md"),
];

pub struct Case {
    pub name: String,
    pub tier: String,
    // The rounds a competent model needs; efficiency compares against it.
    pub par_rounds: u32,
    // Verification fixtures: implementing files written under the temp deliverable.
    pub deliverable: BTreeMap<String, String>,
    pub task_type: String,
    pub target: String,
    pub docs: BTreeMap<String, String>,
    pub entities: BTreeMap<String, Value>,
    pub requirements: BTreeMap<String, Value>,
    pub coverage: BTreeMap<String, String>,
    pub lint: Linting,
    pub checks: Vec<(String, Value)>,
}

// The results file compares only within one case set: hash every embedded case
// definition. Mirrors docs/benchmark/benchmark.md#results-file.
pub fn case_set_hash() -> String {
    let blocks: Vec<String> = CASE_FILES.iter().flat_map(|f| yaml_blocks(f)).collect();
    hash_hex(&blocks.join("\n---\n"))
}

// Pull every fenced ```yaml block out of a markdown file.
// A case whose fixture holds a fenced code block needs a four-backtick outer fence
// (CommonMark's own rule); the closer must match the opener's length, so inner
// three-backtick fences pass through as content.
fn yaml_blocks(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<(String, usize)> = None;
    for line in md.lines() {
        let t = line.trim();
        match &mut cur {
            None => {
                let ticks = t.chars().take_while(|c| *c == '`').count();
                if ticks >= 3 && t[ticks..].trim() == "yaml" {
                    cur = Some((String::new(), ticks));
                }
            }
            Some((buf, ticks)) => {
                if !t.is_empty() && t.chars().all(|c| c == '`') && t.len() >= *ticks {
                    let (b, _) = cur.take().unwrap();
                    out.push(b);
                } else {
                    buf.push_str(line);
                    buf.push('\n');
                }
            }
        }
    }
    out
}

pub fn parse_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for file in CASE_FILES {
        for block in yaml_blocks(file) {
            let v: Value = match serde_norway::from_str(&block) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("jazyk: bad case yaml: {}", e);
                    continue;
                }
            };
            let obj = |x: &Value| -> BTreeMap<String, Value> {
                x.as_object()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default()
            };
            let strs = |x: &Value| -> Vec<String> {
                x.as_array()
                    .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                    .unwrap_or_default()
            };
            let tier = v["tier"].as_str().unwrap_or("extraction").to_string();
            let default_par = match tier.as_str() {
                "review" => 3,
                "generation" => 10,
                "verification" => 1,
                _ => 6,
            };
            cases.push(Case {
                name: v["name"].as_str().unwrap_or("unnamed").to_string(),
                par_rounds: v["par"]["rounds"].as_u64().unwrap_or(default_par) as u32,
                deliverable: obj(&v["given"]["deliverable"])
                    .into_iter()
                    .map(|(k, t)| (k, t.as_str().unwrap_or_default().to_string()))
                    .collect(),
                tier,
                task_type: v["task"]["type"].as_str().unwrap_or_default().to_string(),
                target: v["task"]["target"].as_str().unwrap_or_default().to_string(),
                docs: obj(&v["given"]["docs"])
                    .into_iter()
                    .map(|(k, t)| (k, t.as_str().unwrap_or_default().to_string()))
                    .collect(),
                entities: obj(&v["given"]["graph"]["entities"]),
                requirements: obj(&v["given"]["graph"]["requirements"]),
                coverage: obj(&v["given"]["graph"]["coverage"])
                    .into_iter()
                    .map(|(k, s)| (k, s.as_str().unwrap_or_default().to_string()))
                    .collect(),
                lint: Linting {
                    warnings: strs(&v["given"]["lint"]["warnings"]),
                    errors: strs(&v["given"]["lint"]["errors"]),
                },
                checks: v["assert"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|c| {
                                c.as_object()
                                    .and_then(|m| m.iter().next())
                                    .map(|(k, v)| (k.clone(), v.clone()))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }
    }
    // A case with no checks is unwinnable and grades a lie (0/0); a fixture whose
    // yaml was truncated by an inner fence is the known way to produce one. Refuse it
    // loudly instead of grading it silently.
    for c in &cases {
        if c.checks.is_empty() {
            eprintln!("jazyk: case `{}` has no checks; its yaml block is broken (inner fence?)", c.name);
        }
    }
    cases.retain(|c| !c.checks.is_empty());
    cases
}

fn source_ref(v: &Value) -> Option<SourceRef> {
    let full = v["section"].as_str()?;
    let (doc, section) = split_section_ref(full)?;
    Some(SourceRef { doc, section, quote: v["quote"].as_str().unwrap_or_default().to_string() })
}

// Seed a sandbox store from a case fixture. The sandbox writes to a throwaway out dir.
pub fn sandbox(case: &Case, tmp: &std::path::Path) -> Store {
    let mut s = Store { out: tmp.to_path_buf(), ..Default::default() };
    for (doc, text) in &case.docs {
        s.docs.insert(
            doc.clone(),
            DocRecord { content_hash: hash_hex(text), sections: crate::md::parse_sections(text), coverage: BTreeMap::new() },
        );
    }
    for (full, state) in &case.coverage {
        if let Some((doc, section)) = split_section_ref(full) {
            if let Some(rec) = s.docs.get_mut(&doc) {
                rec.coverage.insert(section, Coverage { state: state.clone(), note: None, claimed_by: None });
            }
        }
    }
    for (id, e) in &case.entities {
        s.graph.entities.insert(
            id.clone(),
            Entity {
                name: e["name"].as_str().unwrap_or_default().to_string(),
                aliases: e["aliases"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                definition: e["definition"].as_str().map(String::from),
                mentions: e["mentions"]
                    .as_array()
                    .map(|a| a.iter().filter_map(source_ref).collect())
                    .unwrap_or_default(),
                ..Default::default()
            },
        );
    }
    for (id, r) in &case.requirements {
        let Some(source) = source_ref(&r["source"]) else { continue };
        s.graph.requirements.insert(
            id.clone(),
            Requirement {
                ears: r["ears"].as_str().unwrap_or_default().to_string(),
                entities: r["entities"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                edges: Vec::new(),
                source,
                confidence: None,
                reasoning: None,
                created: None,
                updated: None,
            },
        );
    }
    s.recompute_relationships();
    s
}

// Check patterns are regular expressions per case.schema.yaml, matched
// case-insensitively. An invalid pattern is a check failure, never a silent pass.
fn compile(pattern: &str) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map_err(|e| format!("bad pattern `{}`: {}", pattern, e))
}

// Resolve a check's entity reference: an id, or a unique exact name/alias match.
fn find_entity(store: &Store, ident: &str) -> Option<String> {
    if store.graph.entities.contains_key(ident) {
        return Some(ident.to_string());
    }
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    store
        .graph
        .entities
        .iter()
        .find(|(_, e)| norm(&e.name) == norm(ident) || e.aliases.iter().any(|a| norm(a) == norm(ident)))
        .map(|(id, _)| id.clone())
}

// Evaluate one check against the resulting store and the staged-mutation count.
// Returns None on pass, or a short failure description.
pub fn eval_check(kind: &str, arg: &Value, store: &Store, staged: usize) -> Option<String> {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    match kind {
        "entityExists" => {
            let want = norm(arg["name"].as_str().unwrap_or_default());
            let found = store.graph.entities.values().any(|e| {
                norm(&e.name) == want || e.aliases.iter().any(|a| norm(a) == want)
            });
            (!found).then(|| format!("no entity named {}", arg["name"]))
        }
        "entityAbsent" => {
            let pat = arg["namePattern"].as_str().unwrap_or_default();
            let re = match compile(pat) {
                Ok(re) => re,
                Err(e) => return Some(e),
            };
            store
                .graph
                .entities
                .values()
                .find(|e| re.is_match(&e.name))
                .map(|e| format!("entity `{}` matches forbidden pattern {}", e.name, pat))
        }
        "entityCount" => {
            let n = store.graph.entities.len();
            if let Some(max) = arg["max"].as_u64() {
                if n as u64 > max {
                    return Some(format!("{} entities, max {}", n, max));
                }
            }
            if let Some(min) = arg["min"].as_u64() {
                if (n as u64) < min {
                    return Some(format!("{} entities, min {}", n, min));
                }
            }
            None
        }
        "requirementExists" => {
            let pat = arg["earsPattern"].as_str().unwrap_or_default();
            let ent = arg["entity"].as_str().unwrap_or_default();
            let re = match compile(pat) {
                Ok(re) => re,
                Err(e) => return Some(e),
            };
            let Some(ent_id) = find_entity(store, ent) else {
                return Some(format!("entity {} not found", ent));
            };
            let found = store.graph.requirements.values().any(|r| {
                re.is_match(&r.ears) && r.entities.iter().any(|e| store.resolve_id(e) == ent_id)
            });
            (!found).then(|| format!("no requirement matching `{}` on {}", pat, ent_id))
        }
        "requirementCount" => {
            let n = match arg["entity"].as_str() {
                Some(ent) => {
                    let Some(ent_id) = find_entity(store, ent) else {
                        return Some(format!("entity {} not found", ent));
                    };
                    store
                        .graph
                        .requirements
                        .values()
                        .filter(|r| r.entities.iter().any(|e| store.resolve_id(e) == ent_id))
                        .count()
                }
                None => store.graph.requirements.len(),
            };
            if let Some(max) = arg["max"].as_u64() {
                if n as u64 > max {
                    return Some(format!("{} requirements, max {}", n, max));
                }
            }
            if let Some(min) = arg["min"].as_u64() {
                if (n as u64) < min {
                    return Some(format!("{} requirements, min {}", n, min));
                }
            }
            None
        }
        "relationshipExists" => {
            let a = arg["a"].as_str().unwrap_or_default();
            let b = arg["b"].as_str().unwrap_or_default();
            let Some(a_id) = find_entity(store, a) else {
                return Some(format!("entity {} not found", a));
            };
            let Some(b_id) = find_entity(store, b) else {
                return Some(format!("entity {} not found", b));
            };
            let want_type = arg["type"].as_str();
            let found = store.graph.relationships.values().any(|r| {
                let members: Vec<String> = r.members.iter().map(|m| store.resolve_id(m).to_string()).collect();
                members.contains(&a_id)
                    && members.contains(&b_id)
                    && want_type.map(|t| r.rel_type == t).unwrap_or(true)
            });
            (!found).then(|| {
                format!(
                    "no {} relationship between {} and {}",
                    want_type.unwrap_or("derived"),
                    a_id,
                    b_id
                )
            })
        }
        "mutationCount" => {
            if let Some(max) = arg["max"].as_u64() {
                if staged as u64 > max {
                    return Some(format!("{} mutations staged, max {}", staged, max));
                }
            }
            if let Some(min) = arg["min"].as_u64() {
                if (staged as u64) < min {
                    return Some(format!("{} mutations staged, min {}", staged, min));
                }
            }
            None
        }
        "diagnosticExists" => {
            let rule = arg["rule"].as_str().unwrap_or_default();
            let subject = arg["subject"].as_str();
            let found = store.graph.diagnostics.values().any(|d| {
                d.lifecycle == "open"
                    && d.rule == rule
                    && subject
                        .map(|want| d.subjects.iter().any(|s| store.resolve_id(s) == store.resolve_id(want)))
                        .unwrap_or(true)
            });
            (!found).then(|| format!("no open {} diagnostic on {}", rule, subject.unwrap_or("any subject")))
        }
        "diagnosticAbsent" => {
            let rule = arg["rule"].as_str().unwrap_or_default();
            store
                .graph
                .diagnostics
                .values()
                .find(|d| d.lifecycle == "open" && d.rule == rule)
                .map(|d| format!("unexpected {} diagnostic: {}", rule, llm::truncate(&d.message, 60)))
        }
        "coverageSet" => {
            let full = arg["section"].as_str().unwrap_or_default();
            let want = arg["state"].as_str().unwrap_or_default();
            let Some((doc, section)) = split_section_ref(full) else {
                return Some(format!("bad section ref {}", full));
            };
            let got = store
                .docs
                .get(&doc)
                .and_then(|r| r.coverage.get(&section))
                .map(|c| c.state.clone())
                .unwrap_or_else(|| "unprocessed".to_string());
            (got != want).then(|| format!("{} coverage is {}, expected {}", full, got, want))
        }
        other => Some(format!("unknown check kind {}", other)),
    }
}


// Tiers with unknown names grade as extraction, the strictest default.
pub fn tier_key(t: &str) -> &'static str {
    match t {
        "review" => "review",
        "generation" => "generation",
        "verification" => "verification",
        _ => "extraction",
    }
}

// Generation and verification checks: deterministic code over the ledger, the files on
// disk, and the exit codes of recorded commands. Mirrors
// docs/benchmark/benchmark.md#deterministic-grading.
pub const WORKFLOW_CHECKS: [&str; 5] =
    ["generationRecorded", "rowPerRequirement", "testsPass", "testFalsifiable", "verdictIs"];

pub fn eval_workflow_check(
    kind: &str,
    arg: &Value,
    store: &Store,
    gs: &crate::gen::GenSettings,
    target: &str,
) -> Option<String> {
    let ledger = crate::gen::Ledger::load(&store.out);
    match kind {
        "generationRecorded" => {
            let slug = crate::gen::slug_of(target);
            match ledger.entities.get(&slug) {
                None => Some("record_generation never landed".into()),
                Some(e) if e.files.is_empty() => Some("the manifest recorded no files".into()),
                Some(e) => {
                    for f in &e.files {
                        if !gs.deliverable.join(f).exists() {
                            return Some(format!("recorded file `{}` does not exist", f));
                        }
                    }
                    if e.fact_hash != crate::gen::fact_hash(store, target) {
                        return Some("the recorded factHash is stale".into());
                    }
                    None
                }
            }
        }
        "rowPerRequirement" => {
            for rid in crate::gen::reqs_of_sorted(store, target) {
                if !ledger.requirements.contains_key(&rid) {
                    return Some(format!("no test row for {}", rid));
                }
            }
            None
        }
        "testsPass" => {
            let mut ran = 0;
            for (rid, row) in &ledger.requirements {
                if row.test.kind != "programmatic" {
                    continue;
                }
                ran += 1;
                match crate::verify::run_programmatic(store, rid, gs) {
                    Ok(r) if r.pass => {}
                    Ok(r) => return Some(format!("{} fails as recorded (exit {})", rid, r.code)),
                    Err(e) => return Some(format!("{}: {}", rid, e)),
                }
            }
            if ran == 0 {
                return Some("no programmatic test rows to run".into());
            }
            None
        }
        "testFalsifiable" => {
            let rid = arg["requirement"].as_str().unwrap_or_default();
            let needle = arg["replace"].as_str().unwrap_or_default();
            let Some(row) = ledger.requirements.get(rid) else {
                return Some(format!("no row for {}", rid));
            };
            if row.test.kind != "programmatic" {
                return Some(format!("{} is not a programmatic row", rid));
            }
            // Break the product (never the test), rerun, restore. A test that still
            // passes with the mandated value gone verifies nothing.
            let slug = crate::gen::slug_of(target);
            let files = ledger.entities.get(&slug).map(|e| e.files.clone()).unwrap_or_default();
            let mut touched: Vec<(std::path::PathBuf, String)> = Vec::new();
            for f in &files {
                if *f == row.test.artifact {
                    continue;
                }
                let p = gs.deliverable.join(f);
                if let Ok(text) = std::fs::read_to_string(&p) {
                    if text.contains(needle) {
                        std::fs::write(&p, text.replace(needle, "BROKEN")).ok();
                        touched.push((p, text));
                    }
                }
            }
            if touched.is_empty() {
                return Some(format!("the mandated value `{}` appears in no product file", needle));
            }
            let verdict = crate::verify::run_programmatic(store, rid, gs);
            for (p, text) in &touched {
                std::fs::write(p, text).ok();
            }
            match verdict {
                Ok(r) if r.pass => Some(format!("{}'s test still passes with the product broken; not falsifiable", rid)),
                Ok(_) => None,
                Err(e) => Some(format!("broken-product rerun errored: {}", e)),
            }
        }
        "verdictIs" => {
            let rid = arg["requirement"].as_str().unwrap_or_default();
            let want = arg["verdict"].as_str().unwrap_or_default();
            match ledger.requirements.get(rid) {
                None => Some(format!("no ledger row for {}", rid)),
                Some(row) if row.verdict == want => None,
                Some(row) => Some(format!("verdict is `{}`, expected `{}`", row.verdict, want)),
            }
        }
        other => Some(format!("unknown workflow check `{}`", other)),
    }
}

// Seed the llm-judge fixture for a verify-requirement case: the implementing files
// under the temp deliverable, the criteria file, and the ledger row the judge reads.
pub fn seed_verification(case: &Case, store: &Store, gs: &crate::gen::GenSettings) -> Result<(), String> {
    let rid = &case.target;
    let r = store.graph.requirements.get(rid).ok_or_else(|| format!("fixture has no requirement {}", rid))?;
    for (f, text) in &case.deliverable {
        let p = gs.deliverable.join(f);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&p, text).map_err(|e| e.to_string())?;
    }
    let files: Vec<String> = case.deliverable.keys().cloned().collect();
    let artifact = format!("criteria/req-{}.md", crate::gen::req_slug(rid));
    let criteria = format!(
        "---\nrequirement: {}\nhash: {}\n---\n\n# Verify {}\n\nStatement: {}\n\n> {}\n\nImplementing files (under the deliverable):\n{}\n\nConfirm the statement holds in the implementation. Verdict contract: reply PASS or FAIL with reasoning.\n",
        rid,
        hash_hex(&r.ears),
        rid,
        r.ears,
        r.source.quote,
        files.iter().map(|f| format!("- {}", f)).collect::<Vec<_>>().join("\n")
    );
    let crit_path = store.out.join("gen").join(&artifact);
    if let Some(parent) = crit_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&crit_path, &criteria).map_err(|e| e.to_string())?;
    let test = crate::gen::TestRef {
        kind: "llm".into(),
        label: "judge".into(),
        artifact,
        name: crate::gen::test_name(rid, &r.ears),
        run: format!("jazyk test {}", rid),
        cwd: ".".into(),
    };
    let mut ledger = crate::gen::Ledger::load(&store.out);
    ledger.requirements.insert(
        rid.clone(),
        crate::gen::ReqRow {
            entity: r.entities.first().cloned().unwrap_or_default(),
            files: files.clone(),
            sites: Vec::new(),
            hashes: crate::gen::RowHashes {
                requirement: hash_hex(&r.ears),
                test: crate::gen::hash_file(&crate::gen::artifact_path(&store.out, gs, &test)),
                files: crate::gen::hash_files(gs, &files),
            },
            test,
            verdict: "none".into(),
            last_run: None,
            exit_code: None,
            evidence: None,
        },
    );
    ledger.save(&store.out);
    Ok(())
}


// One case as a turn on the generic loop: the same codecs the embedded agent runs,
// dispatching into a ToolSession over the sandbox. What run_turn used to be, scoped
// to grading. Mirrors docs/benchmark/benchmark.md#runs.
fn run_case_turn(
    llm: &Llm,
    snapshot: Store,
    item: &crate::model::WorkItem,
    limits: &Limits,
    lint: &crate::project::Linting,
    gs: &crate::gen::GenSettings,
    transcript: Option<&std::path::Path>,
) -> (Vec<crate::store::Op>, u32, Option<String>) {
    use crate::acp::agent::agent_loop::{self, AgentEvent, LoopArgs, Stop};
    use crate::acp::agent::mcp_client::GenericTool;
    use crate::tools::{catalog, toolset, ToolSession, WorkScope};
    let scope = match item.task.as_str() {
        "reconcile-doc" => WorkScope {
            task: item.task.clone(),
            doc: Some(item.target.clone()),
            target: item.target.clone(),
            target_sections: item.dirty_sections.clone(),
            stale_anchors: item.stale_anchors.clone(),
        },
        _ => WorkScope {
            task: item.task.clone(),
            doc: None,
            target: item.target.clone(),
            target_sections: Vec::new(),
            stale_anchors: Vec::new(),
        },
    };
    let (system, pack) = crate::turn::task_prompt(&snapshot, item, limits, lint, gs);
    let names = toolset(&item.task);
    let tools: Vec<GenericTool> = catalog()
        .iter()
        .filter(|t| names.contains(&t.name))
        .map(|t| GenericTool {
            name: t.name.to_string(),
            description: t.description.to_string(),
            parameters: t.parameters.clone(),
        })
        .collect();
    let session = std::cell::RefCell::new({
        let mut s = ToolSession::new(snapshot, scope, limits.turn_mutations, limits.context_budget);
        s.gen = gs.clone();
        s.caller = crate::feedback::Caller { source: "benchmark".into(), target: item.target.clone(), ..Default::default() };
        s
    });
    let mut history = vec![serde_json::json!({"role": "user", "content": format!("{}\n\n{}", system, pack)})];
    let rounds = std::cell::Cell::new(0u32);
    let mut dispatch = |name: &str, args: &serde_json::Value| -> Result<String, String> {
        match session.borrow_mut().dispatch(name, args) {
            Ok(v) => Ok(v.to_string()),
            Err(e) => Err(e.to_value().to_string()),
        }
    };
    let mut emit = |ev: AgentEvent| {
        if let AgentEvent::Usage { .. } = ev {
            rounds.set(rounds.get() + 1);
        }
    };
    let round_budget = limits.turn_rounds.max(item.dirty_sections.len() as u32 * 8);
    let mut stop = agent_loop::run_loop(LoopArgs {
        llm,
        history: &mut history,
        tools: &tools,
        dispatch: &mut dispatch,
        emit: &mut emit,
        // `done` ends the turn: the cancel check is how the loop learns it.
        cancelled: &|| session.borrow().done.is_some(),
        max_rounds: round_budget,
        label: format!("bench {}", item.target),
    });
    // The production client sends one mid-task reminder when a turn ends in prose
    // without finishing (docs/frontends/acp.md#worker-sessions); the benchmark
    // client mirrors it so a graded turn faces the same harness a real one does.
    if matches!(stop, Stop::EndTurn) && session.borrow().done.is_none() {
        history.push(serde_json::json!({"role": "user", "content":
            "The task is not finished: nothing has committed. Continue with the tool calls the instructions name, then finish with done."}));
        stop = agent_loop::run_loop(LoopArgs {
            llm,
            history: &mut history,
            tools: &tools,
            dispatch: &mut dispatch,
            emit: &mut emit,
            cancelled: &|| session.borrow().done.is_some(),
            max_rounds: round_budget,
            label: format!("bench {} (reminded)", item.target),
        });
    }
    let mut s = session.into_inner();
    let failed = if s.done.is_some() {
        None
    } else {
        match stop {
            Stop::Error(e) => Some(e),
            _ => {
                // Graph turns land staged work or fail; generation and binding land
                // in the ledger and the deliverable, so their checks are the judge.
                if s.finish_implicit("(implicit: the turn ended without done)")
                    || matches!(item.task.as_str(), "generate-entity" | "bind-requirement")
                {
                    None
                } else {
                    Some("the turn ended without done and nothing valid was staged".to_string())
                }
            }
        }
    };
    // The full exchange, kept beside the scores: a failed check is diagnosed from
    // what the model saw and said. Mirrors docs/benchmark/benchmark.md#running-a-subset.
    if let Some(path) = transcript {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(path, serde_json::to_string_pretty(&history).unwrap_or_default()).ok();
    }
    (std::mem::take(&mut s.staged), rounds.get(), failed)
}

pub fn run(llm: &Llm, out: &std::path::Path) -> i32 {
    run_filtered(llm, out, &[])
}

// `jazyk benchmark [case...]`: grade only the named cases. A filtered run is for
// iterating on one failure; it never lands in the machine-wide history.
// Mirrors docs/benchmark/benchmark.md#running-a-subset.
pub fn run_filtered(llm: &Llm, out: &std::path::Path, filter: &[String]) -> i32 {
    run_traced_filtered(llm, out, &Trace::stderr(TraceLevel::Quiet), filter)
}

// The GUI's entry: per-case lines reach the job's trace as notes, so a running grade
// shows progress instead of a spinner. Mirrors docs/frontends/gui.md#benchmarks.
pub fn run_traced(llm: &Llm, out: &std::path::Path, progress: &Trace) -> i32 {
    run_traced_filtered(llm, out, progress, &[])
}

fn run_traced_filtered(llm: &Llm, out: &std::path::Path, progress: &Trace, filter: &[String]) -> i32 {
    let mut cases = parse_cases();
    if cases.is_empty() {
        eprintln!("jazyk: no benchmark cases parsed");
        return 2;
    }
    if !filter.is_empty() {
        let known: Vec<String> = cases.iter().map(|c| c.name.clone()).collect();
        for f in filter {
            if !known.contains(f) {
                eprintln!("jazyk: unknown case `{}`; available: {}", f, known.join(", "));
                return 2;
            }
        }
        cases.retain(|c| filter.contains(&c.name));
    }
    let limits = Limits::default();
    let trace = Trace::stderr(TraceLevel::Quiet);
    println!("jazyk benchmark — model {} at {}", llm.model, llm.base_url);
    // One tiny completion before grading: a dead or misrouted endpoint fails one
    // probe, not every case under both codecs.
    if let Err(e) = llm.chat("Reply with the single word: ok", "ok?", "bench preflight", "preflight") {
        println!("
verdict: unmeasured  the endpoint never produced a completion ({})", e);
        return 2;
    }
    let mut any_usable = false;
    let mut codec_reports: Vec<(String, Value)> = Vec::new();

    for (codec_name, mode) in [("native", 1u8), ("text", 2u8)] {
        let started = std::time::Instant::now();
        let tokens_before = llm::tokens_spent();
        // Per-tier perfection (drives verdicts) and per-tier score sums (the scale).
        let mut tier_ok: BTreeMap<&str, bool> = BTreeMap::new();
        let mut tier_sum: BTreeMap<&str, (f64, usize)> = BTreeMap::new();
        let mut checks_passed = 0usize;
        let mut checks_total = 0usize;
        let mut eff_sum = 0.0f64;
        let mut eff_n = 0usize;
        let mut case_results: Vec<(String, Value)> = Vec::new();
        // A codec where no turn ever produces a completion is unmeasured, not graded.
        // Mirrors docs/benchmark/benchmark.md#report.
        let mut any_completion = false;
        let mut first_abort: Option<String> = None;
        println!("\ncodec: {}", codec_name);

        for case in &cases {
            llm::set_tools_mode(mode);
            let tmp = std::env::temp_dir().join(format!("jazyk-bench-{}-{}", std::process::id(), case.name));
            std::fs::remove_dir_all(&tmp).ok();
            let mut store = sandbox(case, &tmp);
            let dirty: Vec<String> = match case.task_type.as_str() {
                "reconcile-doc" => store
                    .docs
                    .get(&case.target)
                    .map(|r| r.sections.keys().cloned().collect())
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            let item = WorkItem {
                task: case.task_type.clone(),
                target: case.target.clone(),
                dirty_sections: dirty,
                stale_anchors: Vec::new(),
            };
            let case_start = std::time::Instant::now();
            let case_tokens_before = llm::tokens_spent();
            // The generation turn writes into a temp deliverable beside the sandbox.
            let gs = crate::gen::GenSettings { deliverable: tmp.join("deliverable"), worker: "agentic".into(), code: Vec::new() };
            std::fs::create_dir_all(&gs.deliverable).ok();
            let mut fail: Option<String> = None;
            let mut aborted = false;
            let (rounds, staged) = if case.task_type == "verify-requirement" {
                // The llm-judge path: no turn, one judgment over a planted row. The
                // judgment runs through a throwaway runner against the sandbox.
                match seed_verification(case, &store, &gs) {
                    Ok(()) => {
                        let sandbox_proj = crate::project::Project {
                            root: tmp.clone(),
                            out: store.out.clone(),
                            ..Default::default()
                        };
                        match crate::acp::runner::AcpRunner::start(&sandbox_proj, llm, &store.out) {
                            Ok(runner) => {
                                if let Err(e) = crate::verify::run_all(&store, &runner, &gs, std::slice::from_ref(&case.target), None, true, &trace) {
                                    fail = Some(format!("judge run failed: {}", e));
                                }
                            }
                            Err(e) => fail = Some(format!("judge runner failed: {}", e)),
                        }
                    }
                    Err(e) => fail = Some(format!("fixture: {}", e)),
                }
                (1u32, 0usize)
            } else {
                let (staged_ops, rounds_n, failed) =
                    run_case_turn(
                        llm,
                        store.clone(),
                        &item,
                        &limits,
                        &case.lint,
                        &gs,
                        Some(&out.join("benchmark").join("trace").join(format!("{}-{}.json", codec_name, case.name))),
                    );
                let staged = staged_ops.len();
                // An aborted turn fails the case with the abort reason. Its checks are
                // skipped and count as failed: an untouched fixture satisfying a check
                // is not evidence. Mirrors docs/benchmark/benchmark.md#runs.
                if let Some(why) = &failed {
                    fail = Some(format!("turn aborted: {}", why));
                    aborted = true;
                    if first_abort.is_none() {
                        first_abort = Some(why.clone());
                    }
                } else {
                    // A native case that silently downgraded mid-turn did not pass natively.
                    if mode == 1 && llm::tools_mode() == 2 {
                        fail = Some("endpoint or model rejected native tool calls".into());
                    }
                    if staged > 0 {
                        store.apply(staged_ops, &item, rounds_n, 0);
                    }
                }
                (rounds_n, staged)
            };
            let case_tokens = llm::tokens_spent() - case_tokens_before;
            any_completion |= case_tokens > 0;
            let mut case_passed = 0usize;
            checks_total += case.checks.len();
            if !aborted {
                for (kind, arg) in &case.checks {
                    let verdict = if WORKFLOW_CHECKS.contains(&kind.as_str()) {
                        eval_workflow_check(kind, arg, &store, &gs, &case.target)
                    } else {
                        eval_check(kind, arg, &store, staged)
                    };
                    match verdict {
                        None => {
                            case_passed += 1;
                            checks_passed += 1;
                        }
                        Some(why) => {
                            if fail.is_none() {
                                fail = Some(format!("{}: {}", kind, why));
                            }
                        }
                    }
                }
            }
            // The scale: this case's score, and its efficiency against par.
            let case_score = if case.checks.is_empty() { 0.0 } else { case_passed as f64 / case.checks.len() as f64 };
            let efficiency = if aborted { 0.0 } else { (case.par_rounds as f64 / rounds.max(1) as f64).min(1.0) };
            if !aborted {
                eff_sum += efficiency;
                eff_n += 1;
            }
            let e = tier_sum.entry(tier_key(&case.tier)).or_insert((0.0, 0));
            e.0 += case_score;
            e.1 += 1;
            std::fs::remove_dir_all(&tmp).ok();
            case_results.push((
                case.name.clone(),
                serde_json::json!({
                    "score": (case_score * 100.0).round() / 100.0,
                    "checks": format!("{}/{}", case_passed, case.checks.len()),
                    "rounds": rounds,
                    "tokens": case_tokens,
                    "parRounds": case.par_rounds,
                    "efficiency": (efficiency * 100.0).round() / 100.0,
                    "fail": fail.clone().unwrap_or_default(),
                }),
            ));
            match &fail {
                None => {
                    println!(
                        "  {:22} 1.00  ({} rounds, par {}, {} tok, {:.0}s)",
                        case.name,
                        rounds,
                        case.par_rounds,
                        case_tokens,
                        case_start.elapsed().as_secs_f32()
                    );
                    progress.line("benchmark", &format!("{} {} 1.00 ({} rounds, {} tok)", codec_name, case.name, rounds, case_tokens));
                }
                Some(why) => {
                    *tier_ok.entry(tier_key(&case.tier)).or_insert(true) = false;
                    println!(
                        "  {:22} {:.2}  {} ({} rounds, {:.0}s)",
                        case.name,
                        case_score,
                        why,
                        rounds,
                        case_start.elapsed().as_secs_f32()
                    );
                    progress.line("benchmark", &format!("{} {} {:.2} {}", codec_name, case.name, case_score, why));
                }
            }
        }

        // No completion ever arrived: nothing was graded, so the codec gets no
        // verdict and no results entry. Mirrors docs/benchmark/benchmark.md#report.
        if !any_completion {
            println!(
                "  verdict: unmeasured  no completion ever arrived ({})",
                first_abort.as_deref().unwrap_or("no turns ran")
            );
            continue;
        }
        // Verdicts per workflow: a tier is held when every one of its cases scored 1.
        // Mirrors docs/benchmark/benchmark.md#report.
        let ok = |t: &str| *tier_ok.get(t).unwrap_or(&true);
        let compilation = match (ok("extraction"), ok("review")) {
            (true, true) => "review",
            (true, false) => "extraction",
            _ => "not-capable",
        };
        let generation = if ok("generation") { "capable" } else { "not-capable" };
        let verification = if ok("verification") { "capable" } else { "not-capable" };
        any_usable |= ok("extraction");
        let secs = started.elapsed().as_secs_f64();
        let tokens = llm::tokens_spent() - tokens_before;
        let throughput = if secs > 0.0 { tokens as f64 / secs } else { 0.0 };
        let tier_score = |t: &str| -> f64 {
            tier_sum.get(t).map(|(sum, n)| if *n == 0 { 0.0 } else { sum / *n as f64 }).unwrap_or(0.0)
        };
        let efficiency = if eff_n == 0 { 0.0 } else { eff_sum / eff_n as f64 };
        println!(
            "  verdicts: compilation {}  generation {}  verification {}",
            compilation, generation, verification
        );
        println!(
            "  scores: extraction {:.2}  review {:.2}  generation {:.2}  verification {:.2}  ({}/{} checks)",
            tier_score("extraction"), tier_score("review"), tier_score("generation"), tier_score("verification"),
            checks_passed, checks_total
        );
        println!(
            "  efficiency {:.2} (rounds vs par)  tokens {}  throughput ~{:.0} tok/s",
            efficiency, tokens, throughput
        );
        codec_reports.push((
            codec_name.to_string(),
            serde_json::json!({
                "verdicts": {"compilation": compilation, "generation": generation, "verification": verification},
                "scores": {
                    "extraction": (tier_score("extraction") * 100.0).round() / 100.0,
                    "review": (tier_score("review") * 100.0).round() / 100.0,
                    "generation": (tier_score("generation") * 100.0).round() / 100.0,
                    "verification": (tier_score("verification") * 100.0).round() / 100.0,
                },
                "checks": format!("{}/{}", checks_passed, checks_total),
                "efficiency": (efficiency * 100.0).round() / 100.0,
                "tokens": tokens,
                "throughput": throughput.round() as u64,
                "cases": case_results.iter().cloned().collect::<BTreeMap<String, Value>>(),
            }),
        ));
    }
    llm::set_tools_mode(0);
    // Both codecs unmeasured: a dead endpoint never overwrites a real grade.
    // Mirrors docs/benchmark/benchmark.md#results-file.
    if codec_reports.is_empty() {
        eprintln!("jazyk: benchmark measured nothing: the endpoint never returned a completion; results not written");
        return 2;
    }
    write_results(out, &llm.model, &codec_reports);
    // A filtered run is a debugging aid, never a capability grade.
    if filter.is_empty() {
        append_history(&llm.model, &llm.base_url, &codec_reports);
    }
    if any_usable {
        0
    } else {
        1
    }
}

// Known results embedded at compile time; curation is manual.
// Mirrors docs/benchmark/benchmark.md#machine-wide-history.
const KNOWN_RESULTS: &str = include_str!("../../docs/benchmark/known-results.yaml");

fn home_history_path() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(|h| std::path::PathBuf::from(h).join(".jazyk").join("benchmarks").join("history.yaml"))
}

// Append one run to the machine-wide history: grades outlive the project that
// produced them. Mirrors docs/benchmark/benchmark.md#machine-wide-history.
pub fn append_history(model: &str, base_url: &str, codec_reports: &[(String, Value)]) {
    let Some(path) = home_history_path() else { return };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut root: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_norway::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({"runs": []}));
    let graded_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = serde_json::json!({
        "model": model,
        "baseUrl": base_url,
        "gradedAt": graded_at,
        "caseSetHash": case_set_hash(),
        "codecs": codec_reports.iter().cloned().collect::<BTreeMap<String, Value>>(),
    });
    if let Some(runs) = root["runs"].as_array_mut() {
        runs.push(entry);
    }
    if let Ok(text) = serde_norway::to_string(&root) {
        std::fs::write(&path, text).ok();
    }
}

// The merged comparison table: embedded known results, the machine-wide history
// (latest per model and codec), and the project's own results file. Every entry
// carries its source and whether its caseSetHash matches this binary's.
pub fn all_results(out: &std::path::Path) -> Value {
    let current_hash = case_set_hash();
    let mut rows: Vec<Value> = Vec::new();
    let mut push = |model: &str, base_url: &str, graded_at: u64, hash: &str, codecs: &Value, source: &str| {
        rows.push(serde_json::json!({
            "model": model,
            "baseUrl": base_url,
            "gradedAt": graded_at,
            "caseSetHash": hash,
            "current": hash == current_hash,
            "codecs": codecs,
            "source": source,
        }));
    };
    if let Ok(known) = serde_norway::from_str::<Value>(KNOWN_RESULTS) {
        for e in known["results"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            push(
                e["model"].as_str().unwrap_or("?"),
                e["baseUrl"].as_str().unwrap_or(""),
                e["gradedAt"].as_u64().unwrap_or(0),
                e["caseSetHash"].as_str().unwrap_or(""),
                &e["codecs"],
                "embedded",
            );
        }
    }
    if let Some(path) = home_history_path() {
        if let Some(hist) = std::fs::read_to_string(&path).ok().and_then(|s| serde_norway::from_str::<Value>(&s).ok()) {
            // Latest per (model, caseSetHash): history is append-only, the table shows tips.
            let mut latest: BTreeMap<(String, String), &Value> = BTreeMap::new();
            for e in hist["runs"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
                let key = (
                    e["model"].as_str().unwrap_or("?").to_string(),
                    e["caseSetHash"].as_str().unwrap_or("").to_string(),
                );
                let newer = latest.get(&key).map(|p| p["gradedAt"].as_u64() <= e["gradedAt"].as_u64()).unwrap_or(true);
                if newer {
                    latest.insert(key, e);
                }
            }
            for e in latest.values() {
                push(
                    e["model"].as_str().unwrap_or("?"),
                    e["baseUrl"].as_str().unwrap_or(""),
                    e["gradedAt"].as_u64().unwrap_or(0),
                    e["caseSetHash"].as_str().unwrap_or(""),
                    &e["codecs"],
                    "history",
                );
            }
        }
    }
    let project = out.join("benchmark").join("results.yaml");
    if let Some(v) = std::fs::read_to_string(&project).ok().and_then(|s| serde_norway::from_str::<Value>(&s).ok()) {
        if let Some(map) = v.as_object() {
            for (model, e) in map {
                push(
                    model,
                    "",
                    e["gradedAt"].as_u64().unwrap_or(0),
                    e["caseSetHash"].as_str().unwrap_or(""),
                    &e["codecs"],
                    "project",
                );
            }
        }
    }
    rows.sort_by(|a, b| b["gradedAt"].as_u64().cmp(&a["gradedAt"].as_u64()));
    serde_json::json!({"caseSetHash": current_hash, "results": rows})
}

// One entry per model in <out>/benchmark/results.yaml, updated in place. Mirrors
// docs/benchmark/benchmark.md#results-file.
fn write_results(out: &std::path::Path, model: &str, codec_reports: &[(String, Value)]) {
    let path = out.join("benchmark").join("results.yaml");
    let mut all: BTreeMap<String, Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_norway::from_str(&s).ok())
        .unwrap_or_default();
    let graded_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    all.insert(
        model.to_string(),
        serde_json::json!({
            "gradedAt": graded_at,
            "caseSetHash": case_set_hash(),
            "codecs": codec_reports.iter().cloned().collect::<BTreeMap<String, Value>>(),
        }),
    );
    if std::fs::create_dir_all(path.parent().unwrap()).is_ok() {
        match serde_norway::to_string(&all) {
            Ok(y) => {
                if let Err(e) = std::fs::write(&path, y) {
                    eprintln!("jazyk: could not write {}: {}", path.display(), e);
                } else {
                    println!("\nresults: {}", path.display());
                }
            }
            Err(e) => eprintln!("jazyk: could not serialize results: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_embedded_cases() {
        let cases = parse_cases();
        assert_eq!(cases.len(), 17); // fifteen files; turn-review and verify-judge hold two blocks each
        // The new tiers parse with their pars and fixtures.
        let gen = cases.iter().find(|c| c.name == "gen-basic").unwrap();
        assert_eq!(gen.tier, "generation");
        assert_eq!(gen.task_type, "generate-entity");
        assert_eq!(gen.par_rounds, 10);
        let steps = cases.iter().find(|c| c.name == "turn-steps").unwrap();
        assert_eq!(steps.checks.len(), 5); // an inner fence must not sever the asserts
        assert!(steps.docs["docs/dedupe.md"].contains("remember the line"), "fixture doc truncated");
        let vj = cases.iter().filter(|c| c.tier == "verification").count();
        assert_eq!(vj, 2);
        let vp = cases.iter().find(|c| c.name == "verify-judge-pass").unwrap();
        assert!(!vp.deliverable.is_empty());
        assert!(cases.iter().any(|c| c.name == "turn-declarative"));
        assert!(cases.iter().any(|c| c.name == "turn-review-clean"));
        let extract = cases.iter().find(|c| c.name == "turn-extract").unwrap();
        assert_eq!(extract.task_type, "reconcile-doc");
        assert_eq!(extract.checks.len(), 6);
        // Tier defaults to extraction; the five review cases declare theirs.
        assert_eq!(extract.tier, "extraction");
        assert_eq!(cases.iter().filter(|c| c.tier == "review").count(), 5);
        let lint = cases.iter().find(|c| c.name == "turn-review-lint").unwrap();
        assert_eq!(lint.lint.warnings.len(), 1);
        // Every embedded pattern must compile, or a case is unwinnable.
        for case in &cases {
            for (kind, arg) in &case.checks {
                let pat = match kind.as_str() {
                    "entityAbsent" => arg["namePattern"].as_str(),
                    "requirementExists" => arg["earsPattern"].as_str(),
                    _ => None,
                };
                if let Some(pat) = pat {
                    assert!(compile(pat).is_ok(), "{}: {}", case.name, pat);
                }
            }
        }
    }

    #[test]
    fn results_file_updates_in_place_per_model() {
        let tmp = std::env::temp_dir().join(format!("jazyk-bench-results-{}", std::process::id()));
        std::fs::remove_dir_all(&tmp).ok();
        write_results(&tmp, "model-a", &[("native".into(), serde_json::json!({"verdict": "review"}))]);
        write_results(&tmp, "model-b", &[("text".into(), serde_json::json!({"verdict": "extraction"}))]);
        write_results(&tmp, "model-a", &[("native".into(), serde_json::json!({"verdict": "extraction"}))]);
        let s = std::fs::read_to_string(tmp.join("benchmark").join("results.yaml")).unwrap();
        let all: BTreeMap<String, Value> = serde_norway::from_str(&s).unwrap();
        assert_eq!(all.len(), 2);
        // The re-grade replaced model-a's entry; model-b survived untouched.
        assert_eq!(all["model-a"]["codecs"]["native"]["verdict"], "extraction");
        assert_eq!(all["model-b"]["codecs"]["text"]["verdict"], "extraction");
        assert_eq!(all["model-a"]["caseSetHash"], all["model-b"]["caseSetHash"]);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn patterns_are_regexes_matched_case_insensitively() {
        let re = |p: &str| compile(p).unwrap();
        assert!(re("empt(y|ies|ied)").is_match("the system shall EMPTY the Cart"));
        assert!(re("^--|/|\\.md").is_match("--api-key"));
        assert!(re("^--|/|\\.md").is_match("src/link.rs"));
        assert!(re("^--|/|\\.md").is_match("notes.md"));
        assert!(!re("^--|/|\\.md").is_match("Shopping Cart"));
        assert!(re("^addProduct$").is_match("addproduct"));
        // An invalid pattern is a check failure, never a silent pass.
        assert!(compile("(unclosed").is_err());
    }

    #[test]
    fn sandbox_seeds_fixture() {
        let cases = parse_cases();
        let converge = cases.iter().find(|c| c.name == "turn-converge").unwrap();
        let tmp = std::env::temp_dir().join("jazyk-bench-test");
        let s = sandbox(converge, &tmp);
        assert!(s.graph.entities.contains_key("ent:cart"));
        assert!(s.graph.requirements.contains_key("req:shop-1"));
        assert_eq!(s.docs["docs/shop.md"].coverage["/shop/checkout"].state, "covered");
        // The fixture's quote must locate in the parsed section, or the case is unwinnable.
        let r = &s.graph.requirements["req:shop-1"];
        assert!(s.quote_locates(&r.source.doc, &r.source.section, &r.source.quote));
    }
}
