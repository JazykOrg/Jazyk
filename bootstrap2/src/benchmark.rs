// The benchmark frontend: decides whether a model is capable of driving compilation
// turns. Runs every case under both codecs in a sandbox store, grades with
// deterministic checks, no LLM judge. Mirrors docs2/benchmark/benchmark.md.
//
// The case definitions ARE the documentation: the fenced yaml blocks in
// docs2/benchmark/cases/*.md are embedded at compile time, one source of truth.
use crate::llm::{self, Llm};
use crate::model::*;
use crate::project::{Limits, Linting};
use crate::store::Store;
use crate::turn::{run_turn, Trace, TraceLevel};
use serde_json::Value;
use std::collections::BTreeMap;

const CASE_FILES: [&str; 11] = [
    include_str!("../../docs2/benchmark/cases/turn-extract.md"),
    include_str!("../../docs2/benchmark/cases/turn-declarative.md"),
    include_str!("../../docs2/benchmark/cases/turn-density.md"),
    include_str!("../../docs2/benchmark/cases/turn-edges.md"),
    include_str!("../../docs2/benchmark/cases/turn-reuse.md"),
    include_str!("../../docs2/benchmark/cases/turn-converge.md"),
    include_str!("../../docs2/benchmark/cases/turn-repair.md"),
    include_str!("../../docs2/benchmark/cases/turn-review.md"),
    include_str!("../../docs2/benchmark/cases/turn-review-duplicate.md"),
    include_str!("../../docs2/benchmark/cases/turn-review-lookalike.md"),
    include_str!("../../docs2/benchmark/cases/turn-review-lint.md"),
];

struct Case {
    name: String,
    tier: String,
    task_type: String,
    target: String,
    docs: BTreeMap<String, String>,
    entities: BTreeMap<String, Value>,
    requirements: BTreeMap<String, Value>,
    coverage: BTreeMap<String, String>,
    lint: Linting,
    checks: Vec<(String, Value)>,
}

// The results file compares only within one case set: hash every embedded case
// definition. Mirrors docs2/benchmark/benchmark.md#results-file.
fn case_set_hash() -> String {
    let blocks: Vec<String> = CASE_FILES.iter().flat_map(|f| yaml_blocks(f)).collect();
    hash_hex(&blocks.join("\n---\n"))
}

// Pull every fenced ```yaml block out of a markdown file.
fn yaml_blocks(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    for line in md.lines() {
        match &mut cur {
            None if line.trim() == "```yaml" => cur = Some(String::new()),
            Some(buf) => {
                if line.trim() == "```" {
                    out.push(cur.take().unwrap());
                } else {
                    buf.push_str(line);
                    buf.push('\n');
                }
            }
            None => {}
        }
    }
    out
}

fn parse_cases() -> Vec<Case> {
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
            cases.push(Case {
                name: v["name"].as_str().unwrap_or("unnamed").to_string(),
                tier: v["tier"].as_str().unwrap_or("extraction").to_string(),
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
    cases
}

fn source_ref(v: &Value) -> Option<SourceRef> {
    let full = v["section"].as_str()?;
    let (doc, section) = split_section_ref(full)?;
    Some(SourceRef { doc, section, quote: v["quote"].as_str().unwrap_or_default().to_string() })
}

// Seed a sandbox store from a case fixture. The sandbox writes to a throwaway out dir.
fn sandbox(case: &Case, tmp: &std::path::Path) -> Store {
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
fn eval_check(kind: &str, arg: &Value, store: &Store, staged: usize) -> Option<String> {
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

pub fn run(llm: &Llm, out: &std::path::Path) -> i32 {
    let cases = parse_cases();
    if cases.is_empty() {
        eprintln!("jazyk: no benchmark cases parsed");
        return 2;
    }
    let limits = Limits::default();
    let trace = Trace { level: TraceLevel::Quiet };
    println!("jazyk benchmark — model {} at {}", llm.model, llm.base_url);
    let mut any_usable = false;
    let mut codec_reports: Vec<(String, Value)> = Vec::new();

    for (codec_name, mode) in [("native", 1u8), ("text", 2u8)] {
        let started = std::time::Instant::now();
        let tokens_before = llm::tokens_spent();
        let mut extraction_ok = true;
        let mut review_ok = true;
        let mut checks_passed = 0usize;
        let mut checks_total = 0usize;
        let mut case_results: Vec<(String, String)> = Vec::new();
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
            let out = run_turn(llm, store.clone(), &item, &limits, &case.lint, &trace);
            let staged = out.session.staged.len();
            let mut fail: Option<String> = None;
            // A native case that silently downgraded mid-turn did not pass natively.
            if mode == 1 && llm::tools_mode() == 2 {
                fail = Some("endpoint or model rejected native tool calls".into());
            }
            if out.failed.is_none() && staged > 0 {
                store.apply(out.session.staged, &item, out.rounds, 0);
            }
            // An aborted turn stages nothing; the checks see the fixture as-is.
            for (kind, arg) in &case.checks {
                checks_total += 1;
                match eval_check(kind, arg, &store, staged) {
                    None => checks_passed += 1,
                    Some(why) => {
                        if fail.is_none() {
                            fail = Some(format!("{}: {}", kind, why));
                        }
                    }
                }
            }
            std::fs::remove_dir_all(&tmp).ok();
            match &fail {
                None => {
                    case_results.push((case.name.clone(), "pass".into()));
                    println!(
                        "  {:22} pass  ({} rounds, {} staged, {:.0}s)",
                        case.name,
                        out.rounds,
                        staged,
                        case_start.elapsed().as_secs_f32()
                    );
                }
                Some(why) => {
                    if case.tier == "review" {
                        review_ok = false;
                    } else {
                        extraction_ok = false;
                    }
                    case_results.push((case.name.clone(), why.clone()));
                    println!(
                        "  {:22} FAIL  {} ({} rounds, {:.0}s)",
                        case.name,
                        why,
                        out.rounds,
                        case_start.elapsed().as_secs_f32()
                    );
                }
            }
        }

        // The verdict is the highest tier whose cases all pass; review implies
        // extraction. Mirrors docs2/benchmark/benchmark.md#report.
        let verdict = match (extraction_ok, review_ok) {
            (true, true) => "review",
            (true, false) => "extraction",
            _ => "not capable",
        };
        any_usable |= extraction_ok;
        let secs = started.elapsed().as_secs_f64();
        let tokens = llm::tokens_spent() - tokens_before;
        let throughput = if secs > 0.0 { tokens as f64 / secs } else { 0.0 };
        println!(
            "  verdict: {}  score {}/{} checks  throughput ~{:.0} tok/s",
            verdict, checks_passed, checks_total, throughput
        );
        codec_reports.push((
            codec_name.to_string(),
            serde_json::json!({
                "verdict": verdict,
                "score": format!("{}/{}", checks_passed, checks_total),
                "throughput": throughput.round() as u64,
                "cases": case_results.iter().cloned().collect::<BTreeMap<String, String>>(),
            }),
        ));
    }
    llm::set_tools_mode(0);
    write_results(out, &llm.model, &codec_reports);
    if any_usable {
        0
    } else {
        1
    }
}

// One entry per model in <out>/benchmark/results.yaml, updated in place. Mirrors
// docs2/benchmark/benchmark.md#results-file.
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
        assert_eq!(cases.len(), 12); // eleven files, turn-review holds two blocks
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
