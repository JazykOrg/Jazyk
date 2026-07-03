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

const CASE_FILES: [&str; 5] = [
    include_str!("../../docs2/benchmark/cases/turn-extract.md"),
    include_str!("../../docs2/benchmark/cases/turn-reuse.md"),
    include_str!("../../docs2/benchmark/cases/turn-converge.md"),
    include_str!("../../docs2/benchmark/cases/turn-repair.md"),
    include_str!("../../docs2/benchmark/cases/turn-review.md"),
];

struct Case {
    name: String,
    task_type: String,
    target: String,
    docs: BTreeMap<String, String>,
    entities: BTreeMap<String, Value>,
    requirements: BTreeMap<String, Value>,
    coverage: BTreeMap<String, String>,
    checks: Vec<(String, Value)>,
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
            cases.push(Case {
                name: v["name"].as_str().unwrap_or("unnamed").to_string(),
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

// Minimal pattern matcher for the check patterns: literals, one level of (a|b|c)
// alternation groups, top-level |, a leading ^ anchor, and \. escapes. Case-insensitive.
// Full regular expressions are deliberately out of scope (no dependencies).
fn expand_alternatives(pat: &str) -> Vec<String> {
    if let (Some(open), Some(close)) = (pat.find('('), pat.find(')')) {
        if open < close {
            let (head, rest) = (&pat[..open], &pat[close + 1..]);
            let mut out = Vec::new();
            for alt in pat[open + 1..close].split('|') {
                for tail in expand_alternatives(rest) {
                    out.push(format!("{}{}{}", head, alt, tail));
                }
            }
            return out;
        }
    }
    vec![pat.to_string()]
}

fn mini_match(pattern: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    for top in split_top_level(pattern) {
        for alt in expand_alternatives(&top) {
            let (anchored, body) = match alt.strip_prefix('^') {
                Some(b) => (true, b),
                None => (false, alt.as_str()),
            };
            let needle = body.replace("\\.", ".").to_lowercase();
            if needle.is_empty() {
                continue;
            }
            let hit = if anchored { text.starts_with(&needle) } else { text.contains(&needle) };
            if hit {
                return true;
            }
        }
    }
    false
}

// Split on | that is not inside a () group.
fn split_top_level(pat: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in pat.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            '|' if depth == 0 => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
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
            store
                .graph
                .entities
                .values()
                .find(|e| mini_match(pat, &e.name))
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
            let ent_id = if store.graph.entities.contains_key(ent) {
                Some(ent.to_string())
            } else {
                store
                    .graph
                    .entities
                    .iter()
                    .find(|(_, e)| norm(&e.name) == norm(ent) || e.aliases.iter().any(|a| norm(a) == norm(ent)))
                    .map(|(id, _)| id.clone())
            };
            let Some(ent_id) = ent_id else {
                return Some(format!("entity {} not found", ent));
            };
            let found = store.graph.requirements.values().any(|r| {
                mini_match(pat, &r.ears) && r.entities.iter().any(|e| store.resolve_id(e) == ent_id)
            });
            (!found).then(|| format!("no requirement matching `{}` on {}", pat, ent_id))
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
            let subject = arg["subject"].as_str().unwrap_or_default();
            let found = store.graph.diagnostics.values().any(|d| {
                d.lifecycle == "open"
                    && d.rule == rule
                    && d.subjects.iter().any(|s| store.resolve_id(s) == store.resolve_id(subject))
            });
            (!found).then(|| format!("no open {} diagnostic on {}", rule, subject))
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

pub fn run(llm: &Llm) -> i32 {
    let cases = parse_cases();
    if cases.is_empty() {
        eprintln!("jazyk: no benchmark cases parsed");
        return 2;
    }
    let limits = Limits::default();
    let lint = Linting::default();
    let trace = Trace { level: TraceLevel::Quiet };
    println!("jazyk benchmark — model {} at {}", llm.model, llm.base_url);
    let mut any_capable = false;

    for (codec_name, mode) in [("native", 1u8), ("text", 2u8)] {
        let started = std::time::Instant::now();
        let tokens_before = llm::tokens_spent();
        let mut passed_cases = 0usize;
        let mut checks_passed = 0usize;
        let mut checks_total = 0usize;
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
            let out = run_turn(llm, store.clone(), &item, &limits, &lint, &trace);
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
                    passed_cases += 1;
                    println!(
                        "  {:16} pass  ({} rounds, {} staged, {:.0}s)",
                        case.name,
                        out.rounds,
                        staged,
                        case_start.elapsed().as_secs_f32()
                    );
                }
                Some(why) => println!(
                    "  {:16} FAIL  {} ({} rounds, {:.0}s)",
                    case.name,
                    why,
                    out.rounds,
                    case_start.elapsed().as_secs_f32()
                ),
            }
        }

        let capable = passed_cases == cases.len();
        any_capable |= capable;
        let secs = started.elapsed().as_secs_f64();
        let tokens = llm::tokens_spent() - tokens_before;
        println!(
            "  verdict: {}  score {}/{} checks ({} of {} cases)  throughput ~{:.0} tok/s",
            if capable { "capable" } else { "not capable" },
            checks_passed,
            checks_total,
            passed_cases,
            cases.len(),
            if secs > 0.0 { tokens as f64 / secs } else { 0.0 }
        );
    }
    llm::set_tools_mode(0);
    if any_capable {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_embedded_cases() {
        let cases = parse_cases();
        assert_eq!(cases.len(), 6); // five files, turn-review holds two blocks
        assert!(cases.iter().any(|c| c.name == "turn-review-clean"));
        let extract = cases.iter().find(|c| c.name == "turn-extract").unwrap();
        assert_eq!(extract.task_type, "reconcile-doc");
        assert_eq!(extract.checks.len(), 6);
    }

    #[test]
    fn mini_match_covers_case_patterns() {
        assert!(mini_match("empt(y|ies|ied)", "the system shall empty the Cart"));
        assert!(mini_match("^--|/|\\.md", "--api-key"));
        assert!(mini_match("^--|/|\\.md", "src/link.rs"));
        assert!(mini_match("^--|/|\\.md", "notes.md"));
        assert!(!mini_match("^--|/|\\.md", "Shopping Cart"));
        assert!(mini_match("places an order", "When the Customer places an order, ..."));
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
