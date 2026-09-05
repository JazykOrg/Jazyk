// The benchmark frontend: decides whether a model is capable of driving compilation
// turns. Runs every case under both codecs in a sandbox store, grades with
// deterministic checks, no LLM judge. Mirrors docs/benchmark/benchmark.md.
//
// The case definitions ARE the documentation: the fenced yaml blocks in
// docs/benchmark/cases/*.md are embedded at compile time, one source of truth.
use crate::llm::{self, Llm};
use crate::model::*;
use crate::project::Linting;
use crate::session::{Trace, TraceLevel};
use crate::store::Store;
use serde_json::Value;
use std::collections::BTreeMap;

const CASE_FILES: [&str; 21] = [
    include_str!("../../docs/benchmark/cases/extract.md"),
    include_str!("../../docs/benchmark/cases/navigation.md"),
    include_str!("../../docs/benchmark/cases/declarative.md"),
    include_str!("../../docs/benchmark/cases/density.md"),
    include_str!("../../docs/benchmark/cases/edges.md"),
    include_str!("../../docs/benchmark/cases/reuse.md"),
    include_str!("../../docs/benchmark/cases/converge.md"),
    include_str!("../../docs/benchmark/cases/repair.md"),
    include_str!("../../docs/benchmark/cases/review.md"),
    include_str!("../../docs/benchmark/cases/review-duplicate.md"),
    include_str!("../../docs/benchmark/cases/review-lookalike.md"),
    include_str!("../../docs/benchmark/cases/review-lint.md"),
    include_str!("../../docs/benchmark/cases/rejudge-pair.md"),
    include_str!("../../docs/benchmark/cases/dedupe-candidates.md"),
    include_str!("../../docs/benchmark/cases/abstract-entity.md"),
    include_str!("../../docs/benchmark/cases/split-view.md"),
    include_str!("../../docs/benchmark/cases/curate-view.md"),
    include_str!("../../docs/benchmark/cases/declare-edges.md"),
    include_str!("../../docs/benchmark/cases/steps.md"),
    include_str!("../../docs/benchmark/cases/gen-basic.md"),
    include_str!("../../docs/benchmark/cases/verify-judge.md"),
];

// The kinds whose case builds the goal from kind and target alone, by their internal
// task names; every other kind derives its goal from the seeded fixture.
// Mirrors docs/benchmark/cases.md#derived-goals.
const ASSEMBLED_TASKS: [&str; 4] = [
    "reconcile-doc",
    "review-entity",
    "generate-entity",
    "verify-requirement",
];

pub struct Case {
    pub name: String,
    pub tier: String,
    // The rounds a competent model needs; efficiency compares against it.
    pub par_rounds: u32,
    // Verification fixtures: implementing files written under the temp deliverable.
    pub deliverable: BTreeMap<String, String>,
    // The goal kind as the case names it.
    pub kind: String,
    pub task_type: String,
    pub target: String,
    pub docs: BTreeMap<String, String>,
    pub entities: BTreeMap<String, Value>,
    pub requirements: BTreeMap<String, Value>,
    pub views: BTreeMap<String, Value>,
    pub coverage: BTreeMap<String, String>,
    pub lint: Linting,
    pub checks: Vec<(String, Value)>,
}

impl Case {
    // Whether the case's goal is derived from the seeded fixture rather than
    // assembled from kind and target. Mirrors docs/benchmark/cases.md#derived-goals.
    pub fn derives_goal(&self) -> bool {
        !ASSEMBLED_TASKS.contains(&self.task_type.as_str())
    }
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
                    .map(|a| {
                        a.iter()
                            .filter_map(|s| s.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let tier = v["tier"].as_str().unwrap_or("extraction").to_string();
            let default_par = match tier.as_str() {
                "review" => 3,
                "structure" => 5,
                "generation" => 10,
                "verification" => 1,
                _ => 6,
            };
            let kind = v["goal"]["kind"].as_str().unwrap_or_default().to_string();
            cases.push(Case {
                name: v["name"].as_str().unwrap_or("unnamed").to_string(),
                par_rounds: v["par"]["rounds"].as_u64().unwrap_or(default_par) as u32,
                deliverable: obj(&v["given"]["deliverable"])
                    .into_iter()
                    .map(|(k, t)| (k, t.as_str().unwrap_or_default().to_string()))
                    .collect(),
                tier,
                // Cases key on goal kinds (docs/benchmark/cases.md#case-format); the
                // harness's internal task names still drive the legacy loop.
                task_type: match kind.as_str() {
                    "reconcile-section" => "reconcile-doc".to_string(),
                    "generate" => "generate-entity".to_string(),
                    "verify" => "verify-requirement".to_string(),
                    other => other.to_string(),
                },
                kind,
                target: v["goal"]["target"].as_str().unwrap_or_default().to_string(),
                docs: obj(&v["given"]["docs"])
                    .into_iter()
                    .map(|(k, t)| (k, t.as_str().unwrap_or_default().to_string()))
                    .collect(),
                entities: obj(&v["given"]["graph"]["entities"]),
                requirements: obj(&v["given"]["graph"]["requirements"]),
                views: obj(&v["given"]["graph"]["views"]),
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
            eprintln!(
                "jazyk: case `{}` has no checks; its yaml block is broken (inner fence?)",
                c.name
            );
        }
    }
    cases.retain(|c| !c.checks.is_empty());
    cases
}

fn source_ref(v: &Value) -> Option<SourceRef> {
    let full = v["section"].as_str()?;
    let (doc, section) = split_section_ref(full)?;
    Some(SourceRef {
        doc,
        section,
        quote: v["quote"].as_str().unwrap_or_default().to_string(),
    })
}

fn str_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

// A fixture entity: name, aliases, definition, mentions, plus stereotype, parent,
// scope, and provenance where the case states them.
// Mirrors docs/benchmark/cases.md#case-format.
fn entity_of(e: &Value) -> Entity {
    let base = Entity::default();
    Entity {
        name: e["name"].as_str().unwrap_or_default().to_string(),
        aliases: str_list(&e["aliases"]),
        definition: e["definition"].as_str().map(String::from),
        mentions: e["mentions"]
            .as_array()
            .map(|a| a.iter().filter_map(source_ref).collect())
            .unwrap_or_default(),
        stereotype: e["stereotype"].as_str().map(String::from),
        parent: e["parent"].as_str().map(String::from),
        scope: e["scope"].as_str().map(String::from).unwrap_or(base.scope),
        provenance: serde_json::from_value::<Provenance>(e["provenance"].clone()).ok(),
        ..base
    }
}

// A fixture requirement: statement, entities, source, plus edges, transition, and
// facets where the case states them. None without a source.
fn requirement_of(r: &Value) -> Option<Requirement> {
    let source = source_ref(&r["source"])?;
    Some(Requirement {
        statement: r["statement"].as_str().unwrap_or_default().to_string(),
        entities: str_list(&r["entities"]),
        edges: serde_json::from_value(r["edges"].clone()).unwrap_or_default(),
        facets: serde_json::from_value(r["facets"].clone()).unwrap_or_default(),
        transition: serde_json::from_value(r["transition"].clone()).ok(),
        source: Some(source),
        ..Default::default()
    })
}

// A fixture view, in the view's own serialized shape. A seeded view is curated.
fn view_of(v: &Value) -> Result<View, String> {
    let mut view: View = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    view.default = false;
    Ok(view)
}

// A sandbox store holding the fixture's documents and coverage, no graph nodes yet.
fn sandbox_docs(case: &Case, tmp: &std::path::Path) -> Store {
    let mut s = Store {
        out: tmp.to_path_buf(),
        ..Default::default()
    };
    for (doc, text) in &case.docs {
        s.docs.insert(
            doc.clone(),
            DocRecord {
                content_hash: hash_hex(text),
                sections: crate::md::parse_sections(text),
                coverage: BTreeMap::new(),
            },
        );
    }
    for (full, state) in &case.coverage {
        if let Some((doc, section)) = split_section_ref(full) {
            if let Some(rec) = s.docs.get_mut(&doc) {
                rec.coverage.insert(
                    section,
                    Coverage {
                        state: state.clone(),
                        note: None,
                        claimed_by: None,
                    },
                );
            }
        }
    }
    s
}

// Seed a sandbox store from a case fixture, the nodes written straight into the
// graph. The sandbox writes to a throwaway out dir.
pub fn sandbox(case: &Case, tmp: &std::path::Path) -> Store {
    let mut s = sandbox_docs(case, tmp);
    for (id, e) in &case.entities {
        s.graph.entities.insert(id.clone(), entity_of(e));
    }
    for (id, r) in &case.requirements {
        if let Some(req) = requirement_of(r) {
            s.graph.requirements.insert(id.clone(), req);
        }
    }
    for (id, v) in &case.views {
        if let Ok(view) = view_of(v) {
            s.graph.views.insert(id.clone(), view);
        }
    }
    crate::derive::recompute_relationships(&mut s);
    s
}

// The fixture as one changeset: entities parents first, then requirements, then
// views. Mirrors docs/benchmark/cases.md#derived-goals.
fn fixture_ops(case: &Case) -> Result<Vec<crate::store::Op>, String> {
    use crate::store::Op;
    let mut ops = Vec::new();
    let mut placed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut pending: Vec<(&String, Entity)> = case
        .entities
        .iter()
        .map(|(id, e)| (id, entity_of(e)))
        .collect();
    while !pending.is_empty() {
        let before = pending.len();
        let (ready, rest): (Vec<_>, Vec<_>) = pending.into_iter().partition(|(_, e)| {
            e.parent
                .as_ref()
                .map(|p| placed.contains(p) || !case.entities.contains_key(p))
                .unwrap_or(true)
        });
        for (id, entity) in ready {
            placed.insert(id.clone());
            ops.push(Op::CreateEntity {
                id: id.clone(),
                entity,
            });
        }
        pending = rest;
        if pending.len() == before {
            let ids: Vec<&str> = pending.iter().map(|(id, _)| id.as_str()).collect();
            return Err(format!("entities with a parent cycle: {}", ids.join(", ")));
        }
    }
    for (id, r) in &case.requirements {
        let requirement =
            requirement_of(r).ok_or_else(|| format!("requirement {} has no source", id))?;
        ops.push(Op::CreateRequirement {
            id: id.clone(),
            requirement,
        });
    }
    for (id, v) in &case.views {
        let view = view_of(v).map_err(|e| format!("view {}: {}", id, e))?;
        ops.push(Op::CreateView {
            id: id.clone(),
            view,
        });
    }
    Ok(ops)
}

// Seed a sandbox through one real commit and derive the case's goal on the board:
// the commit writes the change records the fixture implies, and the goal carries the
// change and hints a build would give it. A fixture the commit refuses, or one that
// derives no such goal, is a fixture error. Mirrors docs/benchmark/cases.md#derived-goals.
pub fn seed_derived(case: &Case, tmp: &std::path::Path) -> Result<(Store, Goal), String> {
    std::fs::create_dir_all(tmp).map_err(|e| e.to_string())?;
    let mut store = sandbox_docs(case, tmp);
    let ops = fixture_ops(case)?;
    let report = store.apply(ops, &crate::store::Commit::store("edit"));
    if !report.skipped.is_empty() {
        return Err(format!(
            "the seeding commit refused: {}",
            report.skipped.join("; ")
        ));
    }
    let proj = crate::project::Project {
        root: tmp.to_path_buf(),
        out: store.out.clone(),
        ..Default::default()
    };
    let control = crate::control::Control::load(&proj, &proj.out);
    let board = crate::board::Board::derive(&store, &proj, &control);
    let id = crate::goals::goal_id(&case.kind, &case.target);
    match board.goals.iter().find(|g| g.id == id) {
        Some(g) => Ok((store, g.clone())),
        None => {
            let same_kind: Vec<&str> = board
                .goals
                .iter()
                .filter(|g| g.kind == case.kind)
                .map(|g| g.id.as_str())
                .collect();
            Err(format!(
                "the fixture derives no {}; {} goals derived: {}",
                id,
                case.kind,
                if same_kind.is_empty() {
                    "(none)".to_string()
                } else {
                    same_kind.join(", ")
                }
            ))
        }
    }
}

// Check patterns are regular expressions per case.schema.yaml, matched
// case-insensitively. An invalid pattern is a check failure, never a silent pass.
fn compile(pattern: &str) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map_err(|e| format!("bad pattern `{}`: {}", pattern, e))
}

// A count against a check's optional `min` and `max`.
fn within(n: usize, arg: &Value, what: &str) -> Option<String> {
    if let Some(max) = arg["max"].as_u64() {
        if n as u64 > max {
            return Some(format!("{} {}, max {}", n, what, max));
        }
    }
    if let Some(min) = arg["min"].as_u64() {
        if (n as u64) < min {
            return Some(format!("{} {}, min {}", n, what, min));
        }
    }
    None
}

// Resolve a check's entity reference: an id, or a unique exact name/alias match.
fn find_entity(store: &Store, ident: &str) -> Option<String> {
    if store.graph.entities.contains_key(ident) {
        return Some(ident.to_string());
    }
    let norm = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    store
        .graph
        .entities
        .iter()
        .find(|(_, e)| {
            norm(&e.name) == norm(ident) || e.aliases.iter().any(|a| norm(a) == norm(ident))
        })
        .map(|(id, _)| id.clone())
}

// Evaluate one check against the resulting store and the staged-mutation count.
// Returns None on pass, or a short failure description.
pub fn eval_check(kind: &str, arg: &Value, store: &Store, staged: usize) -> Option<String> {
    let norm = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    match kind {
        "entityExists" => {
            let want = norm(arg["name"].as_str().unwrap_or_default());
            let found = store
                .graph
                .entities
                .values()
                .any(|e| norm(&e.name) == want || e.aliases.iter().any(|a| norm(a) == want));
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
            let pat = arg["statementPattern"].as_str().unwrap_or_default();
            let ent = arg["entity"].as_str().unwrap_or_default();
            let re = match compile(pat) {
                Ok(re) => re,
                Err(e) => return Some(e),
            };
            let Some(ent_id) = find_entity(store, ent) else {
                return Some(format!("entity {} not found", ent));
            };
            let found = store.graph.requirements.values().any(|r| {
                re.is_match(&r.statement)
                    && r.entities.iter().any(|e| store.resolve_id(e) == ent_id)
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
                let members: Vec<String> = r
                    .members
                    .iter()
                    .map(|m| store.resolve_id(m).to_string())
                    .collect();
                members.contains(&a_id)
                    && members.contains(&b_id)
                    && want_type
                        .map(|t| r.contributions.iter().any(|c| c.r#type == t))
                        .unwrap_or(true)
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
            let mut wanted: Vec<String> = str_list(&arg["subjects"]);
            if let Some(s) = arg["subject"].as_str() {
                wanted.push(s.to_string());
            }
            let found = store.graph.diagnostics.values().any(|d| {
                d.lifecycle == "open"
                    && d.rule == rule
                    && wanted.iter().all(|want| {
                        d.subjects
                            .iter()
                            .any(|s| store.resolve_id(s) == store.resolve_id(want))
                    })
            });
            (!found).then(|| {
                format!(
                    "no open {} diagnostic on {}",
                    rule,
                    if wanted.is_empty() {
                        "any subject".to_string()
                    } else {
                        wanted.join(" and ")
                    }
                )
            })
        }
        "entityNameCount" => {
            let pat = arg["namePattern"].as_str().unwrap_or_default();
            let re = match compile(pat) {
                Ok(re) => re,
                Err(e) => return Some(e),
            };
            let n = store
                .graph
                .entities
                .values()
                .filter(|e| re.is_match(&e.name))
                .count();
            within(n, arg, &format!("entities named like `{}`", pat))
        }
        "nodeExists" => {
            let id = arg["id"].as_str().unwrap_or_default();
            let live = !store.graph.redirects.contains_key(id)
                && (store.graph.entities.contains_key(id)
                    || store.graph.requirements.contains_key(id)
                    || store.graph.views.contains_key(id));
            (!live).then(|| format!("no node {}", id))
        }
        "edgeDeclared" | "edgeAbsent" => {
            let rid = arg["requirement"].as_str().unwrap_or_default();
            let Some(r) = store.graph.requirements.get(store.resolve_id(rid)) else {
                return Some(format!("no requirement {}", rid));
            };
            let Some(a) = find_entity(store, arg["a"].as_str().unwrap_or_default()) else {
                return Some(format!("entity {} not found", arg["a"]));
            };
            let Some(b) = find_entity(store, arg["b"].as_str().unwrap_or_default()) else {
                return Some(format!("entity {} not found", arg["b"]));
            };
            let want_type = arg["type"].as_str();
            let ends = |e: &ReqEdge| {
                (
                    store.resolve_id(&e.a).to_string(),
                    store.resolve_id(&e.b).to_string(),
                )
            };
            if kind == "edgeDeclared" {
                let found = r.edges.iter().any(|e| {
                    ends(e) == (a.clone(), b.clone())
                        && want_type
                            .map(|t| e.rel_type.as_deref() == Some(t))
                            .unwrap_or(true)
                });
                (!found).then(|| {
                    format!(
                        "{} declares no {} edge {} -> {}",
                        rid,
                        want_type.unwrap_or("typed"),
                        a,
                        b
                    )
                })
            } else {
                r.edges
                    .iter()
                    .find(|e| {
                        let (x, y) = ends(e);
                        (x == a && y == b) || (x == b && y == a)
                    })
                    .map(|e| {
                        format!(
                            "{} declares a {} edge between {} and {}",
                            rid,
                            e.rel_type.as_deref().unwrap_or("typeless"),
                            a,
                            b
                        )
                    })
            }
        }
        "childCount" => {
            let target = arg["parent"].as_str().unwrap_or_default();
            let level = if crate::board::scope_target(target).is_some() {
                target.to_string()
            } else {
                match find_entity(store, target) {
                    Some(id) => id,
                    None => return Some(format!("entity {} not found", target)),
                }
            };
            let n = crate::board::level_members(store, &level).len();
            within(n, arg, &format!("children of {}", level))
        }
        "parentIs" => {
            let ent = arg["entity"].as_str().unwrap_or_default();
            let want = arg["parent"].as_str().unwrap_or_default();
            let Some(id) = find_entity(store, ent) else {
                return Some(format!("entity {} not found", ent));
            };
            let Some(parent) = find_entity(store, want) else {
                return Some(format!("entity {} not found", want));
            };
            let got = store.graph.entities[&id]
                .parent
                .as_deref()
                .map(|p| store.resolve_id(p).to_string());
            (got.as_deref() != Some(parent.as_str())).then(|| {
                format!(
                    "{} sits under {}, expected {}",
                    id,
                    got.unwrap_or_else(|| "no parent".to_string()),
                    parent
                )
            })
        }
        "groupingOf" => {
            let mut members: Vec<String> = Vec::new();
            for m in str_list(&arg["members"]) {
                match find_entity(store, &m) {
                    Some(id) => members.push(id),
                    None => return Some(format!("entity {} not found", m)),
                }
            }
            let want: std::collections::BTreeSet<String> = members.iter().cloned().collect();
            let re = match arg["namePattern"].as_str().map(compile) {
                Some(Ok(re)) => Some(re),
                Some(Err(e)) => return Some(e),
                None => None,
            };
            let found = store.graph.entities.iter().any(|(id, e)| {
                let Some(Provenance::Derived { from, .. }) = &e.provenance else {
                    return false;
                };
                let named: std::collections::BTreeSet<String> =
                    from.iter().map(|f| store.resolve_id(f).to_string()).collect();
                let children: std::collections::BTreeSet<String> =
                    crate::board::level_members(store, id).into_iter().collect();
                named == want
                    && children == want
                    && re.as_ref().map(|re| re.is_match(&e.name)).unwrap_or(true)
            });
            (!found).then(|| {
                format!(
                    "no grouping derived from exactly [{}]{}",
                    members.join(", "),
                    arg["namePattern"]
                        .as_str()
                        .map(|p| format!(" named like `{}`", p))
                        .unwrap_or_default()
                )
            })
        }
        "viewExists" => {
            let want_kind = arg["kind"].as_str();
            let excluding = arg["excluding"].as_str();
            let re = match arg["titlePattern"].as_str().map(compile) {
                Some(Ok(re)) => Some(re),
                Some(Err(e)) => return Some(e),
                None => None,
            };
            let found = store.graph.views.iter().any(|(id, v)| {
                excluding != Some(id.as_str())
                    && want_kind.map(|k| v.kind == k).unwrap_or(true)
                    && re.as_ref().map(|re| re.is_match(&v.title)).unwrap_or(true)
            });
            (!found).then(|| {
                format!(
                    "no {} view{}{}",
                    want_kind.unwrap_or("other"),
                    arg["titlePattern"]
                        .as_str()
                        .map(|p| format!(" titled like `{}`", p))
                        .unwrap_or_default(),
                    excluding
                        .map(|x| format!(" besides {}", x))
                        .unwrap_or_default()
                )
            })
        }
        "viewMember" | "viewExcludes" | "viewMemberOrder" | "membersAccounted" => {
            let vid = arg["view"].as_str().unwrap_or_default();
            let Some(v) = store.graph.views.get(store.resolve_id(vid)) else {
                return Some(format!("no view {}", vid));
            };
            let resolve = |x: &str| store.resolve_id(x).to_string();
            let members: Vec<String> = v.members.iter().map(|m| resolve(m)).collect();
            let noted = |x: &Exclusion| {
                let n = x.note.trim().to_lowercase();
                !n.is_empty() && !["none", "n/a", "na", "-", "null"].contains(&n.as_str())
            };
            match kind {
                "viewMember" => {
                    let m = resolve(arg["member"].as_str().unwrap_or_default());
                    if v.excluded.iter().any(|x| resolve(&x.id) == m) {
                        return Some(format!("{} is excluded from {}", m, vid));
                    }
                    (!members.contains(&m)).then(|| format!("{} is not a member of {}", m, vid))
                }
                "viewExcludes" => {
                    let m = resolve(arg["member"].as_str().unwrap_or_default());
                    match v.excluded.iter().find(|x| resolve(&x.id) == m) {
                        None => Some(format!("{} is not excluded from {}", m, vid)),
                        Some(x) if !noted(x) => {
                            Some(format!("{} is excluded from {} without a note", m, vid))
                        }
                        Some(_) => None,
                    }
                }
                "viewMemberOrder" => {
                    let before = resolve(arg["before"].as_str().unwrap_or_default());
                    let after = resolve(arg["after"].as_str().unwrap_or_default());
                    let pos = |m: &str| members.iter().position(|x| x == m);
                    match (pos(&before), pos(&after)) {
                        (Some(i), Some(j)) if i < j => None,
                        (Some(_), Some(_)) => {
                            Some(format!("{} comes after {} in {}", before, after, vid))
                        }
                        (None, _) => Some(format!("{} is not a member of {}", before, vid)),
                        (_, None) => Some(format!("{} is not a member of {}", after, vid)),
                    }
                }
                _ => {
                    for m in str_list(&arg["members"]) {
                        let m = resolve(&m);
                        let kept = members.contains(&m)
                            || v.excluded.iter().any(|x| resolve(&x.id) == m && noted(x))
                            || v.collapse.iter().any(|c| store.is_ancestor(c, &m))
                            || store
                                .graph
                                .views
                                .iter()
                                .any(|(oid, o)| oid != vid && o.members.iter().any(|x| resolve(x) == m));
                        if !kept {
                            return Some(format!(
                                "{} is dropped from {} without a sub-view, a collapse, or an exclusion note",
                                m, vid
                            ));
                        }
                    }
                    None
                }
            }
        }
        "viewWithinLimit" => {
            let vid = arg["view"].as_str().unwrap_or_default();
            let limit = arg["limit"].as_str().unwrap_or_default();
            if crate::limits::limit(limit).is_none() {
                return Some(format!("unknown limit {}", limit));
            }
            if !store.graph.views.contains_key(store.resolve_id(vid)) {
                return Some(format!("no view {}", vid));
            }
            crate::derive::threshold_crossings(store)
                .into_iter()
                .find(|c| c.subject == store.resolve_id(vid) && c.limit == limit)
                .map(|c| format!("{}: {} {} > {}", vid, c.count, limit, c.soft))
        }
        "diagnosticAbsent" => {
            let rule = arg["rule"].as_str().unwrap_or_default();
            store
                .graph
                .diagnostics
                .values()
                .find(|d| d.lifecycle == "open" && d.rule == rule)
                .map(|d| {
                    format!(
                        "unexpected {} diagnostic: {}",
                        rule,
                        llm::truncate(&d.message, 60)
                    )
                })
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
        "structure" => "structure",
        "generation" => "generation",
        "verification" => "verification",
        _ => "extraction",
    }
}

// Generation and verification checks: deterministic code over the ledger, the files on
// disk, and the exit codes of recorded commands. Mirrors
// docs/benchmark/benchmark.md#deterministic-grading.
pub const WORKFLOW_CHECKS: [&str; 5] = [
    "generationRecorded",
    "rowPerRequirement",
    "testsPass",
    "testFalsifiable",
    "verdictIs",
];

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
            let files = ledger
                .entities
                .get(&slug)
                .map(|e| e.files.clone())
                .unwrap_or_default();
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
                return Some(format!(
                    "the mandated value `{}` appears in no product file",
                    needle
                ));
            }
            let verdict = crate::verify::run_programmatic(store, rid, gs);
            for (p, text) in &touched {
                std::fs::write(p, text).ok();
            }
            match verdict {
                Ok(r) if r.pass => Some(format!(
                    "{}'s test still passes with the product broken; not falsifiable",
                    rid
                )),
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
pub fn seed_verification(
    case: &Case,
    store: &Store,
    gs: &crate::gen::GenSettings,
) -> Result<(), String> {
    let rid = &case.target;
    let r = store
        .graph
        .requirements
        .get(rid)
        .ok_or_else(|| format!("fixture has no requirement {}", rid))?;
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
        hash_hex(&r.statement),
        rid,
        r.statement,
        r.source.as_ref().map(|s| s.quote.as_str()).unwrap_or_default(),
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
        name: crate::gen::test_name(rid, &r.statement),
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
                requirement: hash_hex(&r.statement),
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
// How a case turn fell short: the session aborted, or the model marked the batch's
// goal failed (docs/benchmark/benchmark.md#runs).
enum TurnFail {
    Abort(String),
    GoalFailed(String),
}

fn run_case_turn(
    llm: &Llm,
    snapshot: Store,
    item: &crate::model::WorkItem,
    _lint: &crate::project::Linting,
    gs: &crate::gen::GenSettings,
    transcript: Option<&std::path::Path>,
    // A snippet runs a goal the board derived, change and hints intact; a case
    // runs the goal its item describes. Mirrors docs/benchmark/benchmark.md#snippets-from-a-real-project.
    derived: Option<&crate::model::Goal>,
) -> (Vec<crate::store::Op>, u32, Option<TurnFail>, crate::store::Commit) {
    use crate::acp::agent::agent_loop::{self, AgentEvent, LoopArgs, Stop};
    use crate::acp::agent::mcp_client::GenericTool;
    use crate::tools::{catalog, toolset, ToolSession, WorkScope};
    let goal = match derived {
        Some(g) => g.clone(),
        None => item.to_goal(crate::model::GoalState::Open),
    };
    let scope = match derived {
        Some(g) => WorkScope::for_batch(&format!("snippet-{}", g.id), std::slice::from_ref(g)),
        None => WorkScope::from_item(item),
    };
    let prompt = crate::session::preview(&snapshot, std::slice::from_ref(&goal));
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
        let mut s = ToolSession::new(
            snapshot,
            scope,
            crate::limits::SESSION_MUTATIONS,
            crate::limits::CONTEXT_BUDGET,
        );
        s.gen = gs.clone();
        s.caller = crate::feedback::Caller {
            source: "benchmark".into(),
            target: item.target.clone(),
            ..Default::default()
        };
        s
    });
    let mut history = vec![serde_json::json!({"role": "user", "content": prompt})];
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
    let round_budget = crate::limits::SESSION_ROUNDS
        .max(item.dirty_sections.len() as u32 * crate::limits::ROUNDS_PER_SECTION);
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
    // A goal the model marked failed fails its case, done or not: every fixture is
    // resolvable by design, so the claim measures the model, not the fixture.
    // Mirrors docs/benchmark/benchmark.md#runs.
    let failed = if let Some((_, reason)) = s.failed_goals().into_iter().next() {
        Some(TurnFail::GoalFailed(reason))
    } else if s.done.is_some() {
        None
    } else {
        match stop {
            Stop::Error(e) => Some(TurnFail::Abort(e)),
            _ => {
                // Graph turns land staged work or fail; generation and binding land
                // in the ledger and the deliverable, so their checks are the judge.
                if s.finish_implicit("(implicit: the turn ended without done)")
                    || matches!(item.task.as_str(), "generate-entity" | "bind-requirement")
                {
                    None
                } else {
                    Some(TurnFail::Abort(
                        "the turn ended without done and nothing valid was staged".to_string(),
                    ))
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
        std::fs::write(
            path,
            serde_json::to_string_pretty(&history).unwrap_or_default(),
        )
        .ok();
    }
    let commit = s.commit(rounds.get(), 0);
    (std::mem::take(&mut s.staged), rounds.get(), failed, commit)
}

pub fn run(llm: &Llm, out: &std::path::Path) -> i32 {
    run_filtered(llm, out, &[])
}

// A project copied whole into a sandbox, except what is never state: the repository,
// build output, and installed packages.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let n = name.to_string_lossy();
        if n == ".git" || n == "target" || n == "node_modules" {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        if src.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

// `jazyk benchmark --project <dir> --goal <id>`: one goal's session from a copy of a
// real project at a real moment. The board derives in the sandbox, the goal runs with
// its derived change and hints, the commit lands in the sandbox, and the project is
// never touched. Mirrors docs/benchmark/benchmark.md#snippets-from-a-real-project.
pub fn run_goal(llm: &Llm, root: &std::path::Path, goal_id: &str, force: bool) -> i32 {
    use crate::model::GoalState;
    let tmp = std::env::temp_dir().join(format!(
        "jazyk-snippet-{}-{}",
        std::process::id(),
        crate::md::slug(goal_id)
    ));
    std::fs::remove_dir_all(&tmp).ok();
    if let Err(e) = copy_tree(root, &tmp) {
        eprintln!(
            "jazyk: cannot copy {} into a sandbox: {}",
            root.display(),
            e
        );
        return 2;
    }
    let proj = crate::project::Project::load(&tmp);
    let mut store = Store::load(&proj.out);
    let (parsed, _) = crate::reconcile::parse_all(&proj);
    store.sync_docs(&parsed);
    // A typed command is its own approval (docs/compiler/control-plane.md): the
    // snippet releases both classes in the sandbox, so a fresh manual-mode project
    // shows its goals open instead of blocked on a release nobody can give here.
    crate::control::release(&proj, &proj.out, Some("compile"));
    crate::control::release(&proj, &proj.out, Some("generate"));
    let control = crate::control::Control::load(&proj, &proj.out);
    let board = crate::board::Board::derive(&store, &proj, &control);
    // A parked goal is the next build's work, and the snippet is that build's
    // session. Mirrors docs/benchmark/benchmark.md#snippets-from-a-real-project.
    let Some(goal) = board
        .goals
        .iter()
        .find(|g| g.id == goal_id && matches!(g.state, GoalState::Open | GoalState::Parked))
        .cloned()
    else {
        let open: Vec<&str> = board
            .goals
            .iter()
            .filter(|g| matches!(g.state, GoalState::Open))
            .map(|g| g.id.as_str())
            .take(40)
            .collect();
        let states: Vec<String> = board
            .goals
            .iter()
            .filter(|g| !matches!(g.state, GoalState::Open))
            .map(|g| format!("{} ({:?})", g.id, g.state))
            .take(12)
            .collect();
        eprintln!(
            "jazyk: `{}` is not an open goal here; open goals: {}{}",
            goal_id,
            if open.is_empty() {
                "(none)".to_string()
            } else {
                open.join(", ")
            },
            if states.is_empty() {
                String::new()
            } else {
                format!("; other goals: {}", states.join(", "))
            }
        );
        return 2;
    };
    if let Some(crate::goals::Ready::Blocked(reason)) = board.readiness.get(goal_id) {
        // Readiness is scheduling, not a precondition of the session
        // (docs/benchmark/benchmark.md): a forced snippet runs the blocked goal
        // and says what it skipped past.
        if force {
            println!("forced: `{}` is not ready ({}); running it anyway", goal_id, reason);
        } else {
            eprintln!(
                "jazyk: `{}` is not ready: {} (--force runs it anyway)",
                goal_id, reason
            );
            return 2;
        }
    }
    let before: std::collections::BTreeSet<String> = board
        .goals
        .iter()
        .filter(|g| matches!(g.state, GoalState::Open))
        .map(|g| g.id.clone())
        .collect();
    let item = WorkItem::from_goal(&goal);
    let gs = crate::gen::GenSettings::resolve(&proj);
    let trace_dir = proj.out.join("benchmark").join("trace");
    std::fs::create_dir_all(&trace_dir).ok();
    let transcript = trace_dir.join(format!("snippet-{}.json", crate::md::slug(goal_id)));
    println!("snippet: {} in {}", goal_id, tmp.display());
    let (ops, rounds, failed, commit) = run_case_turn(
        llm,
        store.clone(),
        &item,
        &Default::default(),
        &gs,
        Some(&transcript),
        Some(&goal),
    );
    let code = match failed {
        Some(TurnFail::Abort(why)) => {
            println!("outcome: aborted: {}", why);
            1
        }
        Some(TurnFail::GoalFailed(reason)) => {
            println!("outcome: the model failed the goal: {}", reason);
            1
        }
        None => {
            println!("staged: {} mutation(s) in {} round(s)", ops.len(), rounds);
            for op in &ops {
                let v = serde_json::to_value(op).unwrap_or_default();
                println!(
                    "  - {} {}",
                    v["op"].as_str().unwrap_or("?"),
                    v["id"].as_str().or(v["keep"].as_str()).unwrap_or("")
                );
            }
            // A resolution with nothing staged still commits, as the build loop
            // does: the journal entry is what closes the goal.
            if !ops.is_empty() || !commit.resolved.is_empty() {
                store.apply(ops, &commit);
                // The build loop clears a resolved goal's change records once the
                // journal holds the resolution; the snippet does the same.
                if commit.resolved.iter().any(|r| r.goal == goal_id) {
                    let ids = board.records_of(goal_id);
                    if !ids.is_empty() {
                        store.clear_changes(&ids);
                    }
                    store.status.parked.retain(|p| p.id != goal_id);
                    store.save_status();
                }
            }
            let after = crate::board::Board::derive(&store, &proj, &control);
            let resolved = !after.open(goal_id);
            println!(
                "outcome: {}",
                if resolved {
                    "resolved"
                } else {
                    "the goal is still open after the commit"
                }
            );
            let opened: Vec<&str> = after
                .goals
                .iter()
                .filter(|g| matches!(g.state, GoalState::Open) && !before.contains(&g.id))
                .map(|g| g.id.as_str())
                .collect();
            println!(
                "opened: {}",
                if opened.is_empty() {
                    "(nothing)".to_string()
                } else {
                    opened.join(", ")
                }
            );
            if resolved {
                0
            } else {
                1
            }
        }
    };
    println!("transcript: {}", transcript.display());
    code
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

fn run_traced_filtered(
    llm: &Llm,
    out: &std::path::Path,
    progress: &Trace,
    filter: &[String],
) -> i32 {
    let mut cases = parse_cases();
    if cases.is_empty() {
        eprintln!("jazyk: no benchmark cases parsed");
        return 2;
    }
    if !filter.is_empty() {
        let known: Vec<String> = cases.iter().map(|c| c.name.clone()).collect();
        for f in filter {
            if !known.contains(f) {
                eprintln!(
                    "jazyk: unknown case `{}`; available: {}",
                    f,
                    known.join(", ")
                );
                return 2;
            }
        }
        cases.retain(|c| filter.contains(&c.name));
    }
    let trace = Trace::stderr(TraceLevel::Quiet);
    println!("jazyk benchmark: model {} at {}", llm.model, llm.base_url);
    // One tiny completion before grading: a dead or misrouted endpoint fails one
    // probe, not every case under both codecs.
    if let Err(e) = llm.chat(
        "Reply with the single word: ok",
        "ok?",
        "bench preflight",
        "preflight",
    ) {
        println!(
            "
verdict: unmeasured  the endpoint never produced a completion ({})",
            e
        );
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
            let tmp = std::env::temp_dir().join(format!(
                "jazyk-bench-{}-{}",
                std::process::id(),
                case.name
            ));
            std::fs::remove_dir_all(&tmp).ok();
            // A derived-goal case seeds through a real commit and takes its goal off
            // the board; a fixture error fails the case before any turn runs.
            // Mirrors docs/benchmark/cases.md#derived-goals.
            let mut fixture_error: Option<String> = None;
            let mut derived_goal: Option<Goal> = None;
            let mut store = if case.derives_goal() {
                match seed_derived(case, &tmp) {
                    Ok((s, g)) => {
                        derived_goal = Some(g);
                        s
                    }
                    Err(e) => {
                        fixture_error = Some(e);
                        sandbox_docs(case, &tmp)
                    }
                }
            } else {
                sandbox(case, &tmp)
            };
            let dirty: Vec<String> = match case.task_type.as_str() {
                "reconcile-doc" => store
                    .docs
                    .get(&case.target)
                    .map(|r| r.sections.keys().cloned().collect())
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            let item = match &derived_goal {
                Some(g) => WorkItem::from_goal(g),
                None => WorkItem {
                    task: case.task_type.clone(),
                    target: case.target.clone(),
                    dirty_sections: dirty,
                    stale_anchors: Vec::new(),
                    proposals: Vec::new(),
                },
            };
            let case_start = std::time::Instant::now();
            let case_tokens_before = llm::tokens_spent();
            // The generation turn writes into a temp deliverable beside the sandbox.
            let gs = crate::gen::GenSettings {
                deliverable: tmp.join("deliverable"),
                worker: "agentic".into(),
                code: Vec::new(),
            };
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
                                if let Err(e) = crate::verify::run_all(
                                    &store,
                                    &runner,
                                    &gs,
                                    std::slice::from_ref(&case.target),
                                    None,
                                    true,
                                    &trace,
                                ) {
                                    fail = Some(format!("judge run failed: {}", e));
                                }
                            }
                            Err(e) => fail = Some(format!("judge runner failed: {}", e)),
                        }
                    }
                    Err(e) => fail = Some(format!("fixture: {}", e)),
                }
                (1u32, 0usize)
            } else if let Some(e) = fixture_error.take() {
                // A fixture the harness cannot seed measures nothing: no turn runs,
                // and the checks count as failed like an abort's.
                fail = Some(format!("fixture: {}", e));
                aborted = true;
                (0u32, 0usize)
            } else {
                let (staged_ops, rounds_n, failed, commit) = run_case_turn(
                    llm,
                    store.clone(),
                    &item,
                    &case.lint,
                    &gs,
                    Some(
                        &out.join("benchmark")
                            .join("trace")
                            .join(format!("{}-{}.json", codec_name, case.name)),
                    ),
                    derived_goal.as_ref(),
                );
                let staged = staged_ops.len();
                match &failed {
                    // An aborted turn fails the case with the abort reason. Its checks
                    // are skipped and count as failed: an untouched fixture satisfying
                    // a check is not evidence. Mirrors docs/benchmark/benchmark.md#runs.
                    Some(TurnFail::Abort(why)) => {
                        fail = Some(format!("turn aborted: {}", why));
                        aborted = true;
                        if first_abort.is_none() {
                            first_abort = Some(why.clone());
                        }
                    }
                    // A goal the model marked failed fails the case with the model's
                    // reason; the checks are skipped and count as failed, as on an
                    // abort. Mirrors docs/benchmark/benchmark.md#runs.
                    Some(TurnFail::GoalFailed(reason)) => {
                        fail = Some(format!("goal marked failed: {}", reason));
                        aborted = true;
                    }
                    None => {
                        // A native case that silently downgraded mid-turn did not pass natively.
                        if mode == 1 && llm::tools_mode() == 2 {
                            fail = Some("endpoint or model rejected native tool calls".into());
                        }
                        if staged > 0 {
                            // A derived goal commits as the session did, its
                            // resolution in the journal; an assembled one as the
                            // legacy loop does.
                            if derived_goal.is_some() {
                                store.apply(staged_ops, &commit);
                            } else {
                                store.apply(staged_ops, &item.commit(rounds_n, 0));
                            }
                        }
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
            let case_score = if case.checks.is_empty() {
                0.0
            } else {
                case_passed as f64 / case.checks.len() as f64
            };
            let efficiency = if aborted {
                0.0
            } else {
                (case.par_rounds as f64 / rounds.max(1) as f64).min(1.0)
            };
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
                    progress.line(
                        "benchmark",
                        &format!(
                            "{} {} 1.00 ({} rounds, {} tok)",
                            codec_name, case.name, rounds, case_tokens
                        ),
                    );
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
                    progress.line(
                        "benchmark",
                        &format!("{} {} {:.2} {}", codec_name, case.name, case_score, why),
                    );
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
        // A tier no case ran under (a filtered run) gets `-`, never a claim.
        // Mirrors docs/benchmark/benchmark.md#report and #running-a-subset.
        let ok = |t: &str| *tier_ok.get(t).unwrap_or(&true);
        let measured = |t: &str| tier_sum.get(t).map(|(_, n)| *n > 0).unwrap_or(false);
        let compilation = if !measured("extraction") {
            "-"
        } else if !ok("extraction") {
            "not-capable"
        } else if measured("review") && ok("review") {
            "review"
        } else {
            "extraction"
        };
        let generation = if !measured("generation") {
            "-"
        } else if ok("generation") {
            "capable"
        } else {
            "not-capable"
        };
        let verification = if !measured("verification") {
            "-"
        } else if ok("verification") {
            "capable"
        } else {
            "not-capable"
        };
        any_usable |= ok("extraction");
        let secs = started.elapsed().as_secs_f64();
        let tokens = llm::tokens_spent() - tokens_before;
        let throughput = if secs > 0.0 {
            tokens as f64 / secs
        } else {
            0.0
        };
        let tier_score = |t: &str| -> f64 {
            tier_sum
                .get(t)
                .map(|(sum, n)| if *n == 0 { 0.0 } else { sum / *n as f64 })
                .unwrap_or(0.0)
        };
        let efficiency = if eff_n == 0 {
            0.0
        } else {
            eff_sum / eff_n as f64
        };
        println!(
            "  verdicts: compilation {}  generation {}  verification {}",
            compilation, generation, verification
        );
        let score_str = |t: &str| {
            if measured(t) {
                format!("{:.2}", tier_score(t))
            } else {
                "-".to_string()
            }
        };
        println!(
            "  scores: extraction {}  review {}  structure {}  generation {}  verification {}  ({}/{} checks)",
            score_str("extraction"),
            score_str("review"),
            score_str("structure"),
            score_str("generation"),
            score_str("verification"),
            checks_passed,
            checks_total
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
                    "extraction": score_json("extraction", &measured, &tier_score),
                    "review": score_json("review", &measured, &tier_score),
                    "structure": score_json("structure", &measured, &tier_score),
                    "generation": score_json("generation", &measured, &tier_score),
                    "verification": score_json("verification", &measured, &tier_score),
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
    std::env::var("HOME").ok().map(|h| {
        std::path::PathBuf::from(h)
            .join(".jazyk")
            .join("benchmarks")
            .join("history.yaml")
    })
}

// Append one run to the machine-wide history: grades outlive the project that
// produced them. Mirrors docs/benchmark/benchmark.md#machine-wide-history.
pub fn append_history(model: &str, base_url: &str, codec_reports: &[(String, Value)]) {
    let Some(path) = home_history_path() else {
        return;
    };
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
    let mut push =
        |model: &str, base_url: &str, graded_at: u64, hash: &str, codecs: &Value, source: &str| {
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
        for e in known["results"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or(&[])
        {
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
        if let Some(hist) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_norway::from_str::<Value>(&s).ok())
        {
            // Latest per (model, caseSetHash): history is append-only, the table shows tips.
            let mut latest: BTreeMap<(String, String), &Value> = BTreeMap::new();
            for e in hist["runs"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
                let key = (
                    e["model"].as_str().unwrap_or("?").to_string(),
                    e["caseSetHash"].as_str().unwrap_or("").to_string(),
                );
                let newer = latest
                    .get(&key)
                    .map(|p| p["gradedAt"].as_u64() <= e["gradedAt"].as_u64())
                    .unwrap_or(true);
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
    if let Some(v) = std::fs::read_to_string(&project)
        .ok()
        .and_then(|s| serde_norway::from_str::<Value>(&s).ok())
    {
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
// A tier score is a number only when at least one of its cases ran; a filtered
// run leaves the rest null instead of a fake 0.0.
fn score_json(t: &str, measured: &dyn Fn(&str) -> bool, tier_score: &dyn Fn(&str) -> f64) -> Value {
    if measured(t) {
        serde_json::json!((tier_score(t) * 100.0).round() / 100.0)
    } else {
        Value::Null
    }
}

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
        // Twenty-one files; review, verify-judge, rejudge-pair, dedupe-candidates,
        // abstract-entity, and declare-edges hold two blocks each.
        assert_eq!(cases.len(), 27);
        // The new tiers parse with their pars and fixtures.
        let gen = cases.iter().find(|c| c.name == "gen-basic").unwrap();
        assert_eq!(gen.tier, "generation");
        assert_eq!(gen.task_type, "generate-entity");
        assert_eq!(gen.par_rounds, 10);
        let steps = cases.iter().find(|c| c.name == "steps").unwrap();
        assert_eq!(steps.checks.len(), 5); // an inner fence must not sever the asserts
        assert!(
            steps.docs["docs/dedupe.md"].contains("remember the line"),
            "fixture doc truncated"
        );
        let vj = cases.iter().filter(|c| c.tier == "verification").count();
        assert_eq!(vj, 2);
        let vp = cases
            .iter()
            .find(|c| c.name == "verify-judge-pass")
            .unwrap();
        assert!(!vp.deliverable.is_empty());
        assert!(cases.iter().any(|c| c.name == "declarative"));
        assert!(cases.iter().any(|c| c.name == "review-clean"));
        let extract = cases.iter().find(|c| c.name == "extract").unwrap();
        assert_eq!(extract.task_type, "reconcile-doc");
        assert_eq!(extract.checks.len(), 6);
        // Tier defaults to extraction; the review and structure cases declare theirs.
        assert_eq!(extract.tier, "extraction");
        assert_eq!(cases.iter().filter(|c| c.tier == "review").count(), 9);
        assert_eq!(cases.iter().filter(|c| c.tier == "structure").count(), 6);
        let lint = cases.iter().find(|c| c.name == "review-lint").unwrap();
        assert_eq!(lint.lint.warnings.len(), 1);
        // The derived-goal kinds keep their kind as the task and carry their views.
        let fan = cases.iter().find(|c| c.name == "abstract-entity").unwrap();
        assert!(fan.derives_goal());
        assert_eq!(fan.kind, "abstract-entity");
        assert_eq!(fan.task_type, "abstract-entity");
        assert_eq!(fan.entities.len(), 12);
        assert_eq!(fan.checks.len(), 9);
        let split = cases.iter().find(|c| c.name == "split-view").unwrap();
        assert_eq!(split.views.len(), 1);
        assert!(!extract.derives_goal());
        assert!(!cases.iter().find(|c| c.name == "review").unwrap().derives_goal());
        // Every embedded pattern must compile, or a case is unwinnable.
        for case in &cases {
            for (kind, arg) in &case.checks {
                let pat = match kind.as_str() {
                    "entityAbsent" | "entityNameCount" | "groupingOf" => {
                        arg["namePattern"].as_str()
                    }
                    "requirementExists" => arg["statementPattern"].as_str(),
                    "viewExists" => arg["titlePattern"].as_str(),
                    _ => None,
                };
                if let Some(pat) = pat {
                    assert!(compile(pat).is_ok(), "{}: {}", case.name, pat);
                }
            }
        }
    }

    fn own_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("jazyk-bench-test-{}-{}", std::process::id(), name));
        std::fs::remove_dir_all(&d).ok();
        d
    }

    // Every derived-goal fixture seeds through the commit path and derives exactly
    // the goal its case names, with the change a build would compute.
    // Mirrors docs/benchmark/cases.md#derived-goals.
    #[test]
    fn derived_cases_seed_and_derive_their_goal() {
        let cases = parse_cases();
        let derived: Vec<&Case> = cases.iter().filter(|c| c.derives_goal()).collect();
        assert_eq!(derived.len(), 10);
        for case in derived {
            let tmp = own_dir(&case.name);
            let (store, goal) = seed_derived(case, &tmp)
                .unwrap_or_else(|e| panic!("{}: {}", case.name, e));
            assert_eq!(goal.kind, case.kind, "{}", case.name);
            assert_eq!(goal.target, case.target, "{}", case.name);
            // Every fixture quote locates, or the case is unwinnable.
            for (id, r) in &store.graph.requirements {
                let s = r.source.as_ref().unwrap();
                assert!(
                    store.quote_locates(&s.doc, &s.section, &s.quote),
                    "{}: {} quote does not locate",
                    case.name,
                    id
                );
            }
            for (id, e) in &store.graph.entities {
                for m in &e.mentions {
                    assert!(
                        store.quote_locates(&m.doc, &m.section, &m.quote),
                        "{}: {} mention does not locate",
                        case.name,
                        id
                    );
                }
            }
            // No check is satisfied by the untouched fixture where the case expects
            // work: the planted trap is real.
            let vacuous = case
                .checks
                .iter()
                .all(|(kind, arg)| eval_check(kind, arg, &store, 0).is_none());
            let expects_no_work = case.name == "dedupe-candidates-separate"
                || case.name == "declare-edges-none";
            assert_eq!(!vacuous, !expects_no_work, "{}: vacuous={}", case.name, vacuous);
            match case.name.as_str() {
                "abstract-entity" => {
                    assert_eq!(goal.change["fan_out"], 12);
                    assert!(!goal.change["candidates"].as_array().unwrap().is_empty());
                    assert!(
                        goal.hints.iter().any(|h| h.starts_with("namesake ent:billing ")),
                        "{:?}",
                        goal.hints
                    );
                }
                "abstract-entity-namesake" => {
                    assert_eq!(goal.change["fan_out"], 13);
                    assert!(
                        goal.hints.iter().any(|h| h.starts_with("namesake ent:checkout ")),
                        "{:?}",
                        goal.hints
                    );
                }
                "split-view" => {
                    assert_eq!(goal.change["limits"][0]["limit"], "participants-per-sequence-view");
                    assert!(
                        goal.hints.iter().any(|h| h.starts_with("break after req:purchase-7")),
                        "{:?}",
                        goal.hints
                    );
                }
                "curate-view" => {
                    let matched: Vec<String> = goal.change["matched"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|m| m["id"].as_str().map(String::from))
                        .collect();
                    assert!(matched.contains(&"ent:refund".to_string()), "{:?}", goal.change);
                    assert!(matched.contains(&"ent:order-1042".to_string()), "{:?}", goal.change);
                }
                "declare-edges" => {
                    assert_eq!(goal.change["entities"].as_array().unwrap().len(), 3);
                }
                "dedupe-candidates" | "dedupe-candidates-separate" => {
                    assert!(goal.change["score"].as_f64().unwrap() >= 0.5);
                }
                "rejudge-pair-contradiction" | "rejudge-pair-duplicate" => {
                    assert!(goal.mandatory);
                }
                _ => {}
            }
            std::fs::remove_dir_all(&tmp).ok();
        }
    }

    fn ent(name: &str, parent: Option<&str>) -> Entity {
        Entity {
            name: name.into(),
            parent: parent.map(String::from),
            ..Default::default()
        }
    }

    fn structure_store() -> Store {
        let mut s = Store::default();
        s.graph.entities.insert("ent:orders".into(), Entity {
            definition: Some("Holds the order area.".into()),
            provenance: Some(Provenance::Derived {
                from: vec!["ent:cart".into(), "ent:pricing".into()],
                reasoning: "the orders document".into(),
            }),
            ..ent("Orders", None)
        });
        s.graph.entities.insert("ent:cart".into(), ent("Cart", Some("ent:orders")));
        s.graph.entities.insert("ent:pricing".into(), ent("Pricing", Some("ent:orders")));
        s.graph.entities.insert("ent:billing".into(), ent("Billing", None));
        s.graph.entities.insert("ent:invoice".into(), ent("Invoice", Some("ent:billing")));
        s.graph.entities.insert("ent:billing-area".into(), ent("Billing Area", None));
        s.graph.requirements.insert("req:a".into(), Requirement {
            statement: "The orders area consists of the cart and pricing.".into(),
            entities: vec!["ent:orders".into(), "ent:cart".into(), "ent:pricing".into()],
            edges: vec![ReqEdge {
                a: "ent:orders".into(),
                b: "ent:cart".into(),
                rel_type: Some("composition".into()),
                cardinality: None,
            }],
            ..Default::default()
        });
        s.graph.views.insert("view:sequence/flow".into(), View {
            kind: "sequence".into(),
            title: "Flow".into(),
            members: vec!["req:a".into()],
            excluded: vec![Exclusion { id: "req:b".into(), note: "belongs to view:sequence/detail".into() },
                           Exclusion { id: "req:c".into(), note: "none".into() }],
            collapse: vec!["ent:orders".into()],
            ..Default::default()
        });
        s.graph.views.insert("view:sequence/detail".into(), View {
            kind: "sequence".into(),
            title: "Flow: Detail".into(),
            members: vec!["req:d".into(), "req:a".into()],
            ..Default::default()
        });
        s
    }

    #[test]
    fn containment_checks_read_the_tree() {
        let s = structure_store();
        let ok = |k: &str, a: Value| eval_check(k, &a, &s, 0);
        assert_eq!(ok("childCount", serde_json::json!({"parent": "ent:orders", "min": 2, "max": 2})), None);
        assert!(ok("childCount", serde_json::json!({"parent": "Orders", "max": 1})).is_some());
        assert_eq!(ok("childCount", serde_json::json!({"parent": "scope:public", "max": 3})), None);
        assert!(ok("childCount", serde_json::json!({"parent": "scope:public", "max": 2})).is_some());
        assert!(ok("childCount", serde_json::json!({"parent": "ent:nope", "max": 2})).is_some());
        assert_eq!(ok("parentIs", serde_json::json!({"entity": "ent:cart", "parent": "Orders"})), None);
        assert!(ok("parentIs", serde_json::json!({"entity": "ent:cart", "parent": "ent:billing"})).is_some());
        assert!(ok("parentIs", serde_json::json!({"entity": "ent:billing", "parent": "ent:orders"})).is_some());
        // A grouping: derived from exactly its members, holding exactly them.
        assert_eq!(ok("groupingOf", serde_json::json!({"members": ["ent:cart", "ent:pricing"]})), None);
        assert_eq!(ok("groupingOf", serde_json::json!({"members": ["ent:cart", "ent:pricing"], "namePattern": "^orders$"})), None);
        assert!(ok("groupingOf", serde_json::json!({"members": ["ent:cart", "ent:pricing"], "namePattern": "^billing"})).is_some());
        assert!(ok("groupingOf", serde_json::json!({"members": ["ent:cart"]})).is_some());
        // A stated parent is no grouping: no derived provenance.
        assert!(ok("groupingOf", serde_json::json!({"members": ["ent:invoice"]})).is_some());
        assert_eq!(ok("entityNameCount", serde_json::json!({"namePattern": "billing", "max": 2})), None);
        assert!(ok("entityNameCount", serde_json::json!({"namePattern": "billing", "max": 1})).is_some());
        assert_eq!(ok("entityNameCount", serde_json::json!({"namePattern": "^billing$", "max": 1})), None);
        assert_eq!(ok("nodeExists", serde_json::json!({"id": "ent:cart"})), None);
        assert_eq!(ok("nodeExists", serde_json::json!({"id": "req:a"})), None);
        assert_eq!(ok("nodeExists", serde_json::json!({"id": "view:sequence/flow"})), None);
        assert!(ok("nodeExists", serde_json::json!({"id": "ent:gone"})).is_some());
    }

    #[test]
    fn edge_checks_read_direction_and_type() {
        let s = structure_store();
        let ok = |k: &str, a: Value| eval_check(k, &a, &s, 0);
        assert_eq!(ok("edgeDeclared", serde_json::json!({"requirement": "req:a", "a": "ent:orders", "b": "ent:cart", "type": "composition"})), None);
        assert_eq!(ok("edgeDeclared", serde_json::json!({"requirement": "req:a", "a": "Orders", "b": "Cart"})), None);
        // Direction counts, and so does the type.
        assert!(ok("edgeDeclared", serde_json::json!({"requirement": "req:a", "a": "ent:cart", "b": "ent:orders"})).is_some());
        assert!(ok("edgeDeclared", serde_json::json!({"requirement": "req:a", "a": "ent:orders", "b": "ent:cart", "type": "dependency"})).is_some());
        assert!(ok("edgeDeclared", serde_json::json!({"requirement": "req:a", "a": "ent:orders", "b": "ent:pricing"})).is_some());
        assert_eq!(ok("edgeAbsent", serde_json::json!({"requirement": "req:a", "a": "ent:cart", "b": "ent:pricing"})), None);
        assert!(ok("edgeAbsent", serde_json::json!({"requirement": "req:a", "a": "ent:cart", "b": "ent:orders"})).is_some());
        assert!(ok("edgeAbsent", serde_json::json!({"requirement": "req:nope", "a": "ent:cart", "b": "ent:orders"})).is_some());
    }

    #[test]
    fn view_checks_read_membership_exclusions_and_order() {
        let s = structure_store();
        let ok = |k: &str, a: Value| eval_check(k, &a, &s, 0);
        assert_eq!(ok("viewExists", serde_json::json!({"kind": "sequence"})), None);
        assert_eq!(ok("viewExists", serde_json::json!({"kind": "sequence", "excluding": "view:sequence/flow"})), None);
        assert_eq!(ok("viewExists", serde_json::json!({"kind": "sequence", "titlePattern": "detail", "excluding": "view:sequence/flow"})), None);
        assert!(ok("viewExists", serde_json::json!({"kind": "sequence", "titlePattern": "^flow$", "excluding": "view:sequence/flow"})).is_some());
        assert!(ok("viewExists", serde_json::json!({"kind": "class"})).is_some());
        assert_eq!(ok("viewMember", serde_json::json!({"view": "view:sequence/flow", "member": "req:a"})), None);
        assert!(ok("viewMember", serde_json::json!({"view": "view:sequence/flow", "member": "req:b"})).is_some());
        assert!(ok("viewMember", serde_json::json!({"view": "view:none", "member": "req:a"})).is_some());
        assert_eq!(ok("viewExcludes", serde_json::json!({"view": "view:sequence/flow", "member": "req:b"})), None);
        // A placeholder note is no note.
        assert!(ok("viewExcludes", serde_json::json!({"view": "view:sequence/flow", "member": "req:c"})).is_some());
        assert!(ok("viewExcludes", serde_json::json!({"view": "view:sequence/flow", "member": "req:a"})).is_some());
        assert_eq!(ok("viewMemberOrder", serde_json::json!({"view": "view:sequence/detail", "before": "req:d", "after": "req:a"})), None);
        assert!(ok("viewMemberOrder", serde_json::json!({"view": "view:sequence/detail", "before": "req:a", "after": "req:d"})).is_some());
        assert!(ok("viewMemberOrder", serde_json::json!({"view": "view:sequence/detail", "before": "req:a", "after": "req:x"})).is_some());
        // Accounted: a member, an excluded id with a note, a member of another
        // view, an id hidden under a collapsed entity; never a placeholder note.
        assert_eq!(ok("membersAccounted", serde_json::json!({"view": "view:sequence/flow", "members": ["req:a", "req:b", "req:d", "ent:cart"]})), None);
        assert!(ok("membersAccounted", serde_json::json!({"view": "view:sequence/flow", "members": ["req:c"]})).is_some());
        assert!(ok("membersAccounted", serde_json::json!({"view": "view:sequence/flow", "members": ["req:zzz"]})).is_some());
    }

    #[test]
    fn view_within_limit_recomputes_the_commits_count() {
        let mut s = Store::default();
        for i in 0..10 {
            s.graph.entities.insert(format!("ent:p{}", i), ent(&format!("P{}", i), None));
        }
        let step = |i: usize| Requirement {
            statement: format!("P{} calls P{}.", i, i + 1),
            entities: vec![format!("ent:p{}", i), format!("ent:p{}", i + 1)],
            edges: vec![ReqEdge {
                a: format!("ent:p{}", i),
                b: format!("ent:p{}", i + 1),
                rel_type: Some("dependency".into()),
                cardinality: None,
            }],
            ..Default::default()
        };
        for i in 0..9 {
            s.graph.requirements.insert(format!("req:s{}", i), step(i));
        }
        s.graph.views.insert("view:sequence/wide".into(), View {
            kind: "sequence".into(),
            title: "Wide".into(),
            members: (0..9).map(|i| format!("req:s{}", i)).collect(),
            ..Default::default()
        });
        s.graph.views.insert("view:sequence/narrow".into(), View {
            kind: "sequence".into(),
            title: "Narrow".into(),
            members: (0..3).map(|i| format!("req:s{}", i)).collect(),
            ..Default::default()
        });
        let ok = |k: &str, a: Value| eval_check(k, &a, &s, 0);
        // Ten participants against a soft limit of eight.
        let why = ok("viewWithinLimit", serde_json::json!({"view": "view:sequence/wide", "limit": "participants-per-sequence-view"}));
        assert!(why.as_deref().is_some_and(|w| w.contains("10 participants-per-sequence-view > 8")), "{:?}", why);
        assert_eq!(ok("viewWithinLimit", serde_json::json!({"view": "view:sequence/narrow", "limit": "participants-per-sequence-view"})), None);
        assert_eq!(ok("viewWithinLimit", serde_json::json!({"view": "view:sequence/wide", "limit": "members-per-flow-view"})), None);
        assert!(ok("viewWithinLimit", serde_json::json!({"view": "view:sequence/wide", "limit": "no-such-limit"})).is_some());
        assert!(ok("viewWithinLimit", serde_json::json!({"view": "view:sequence/none", "limit": "edges-per-view"})).is_some());
    }

    #[test]
    fn diagnostic_exists_takes_every_listed_subject() {
        let mut s = Store::default();
        s.graph.diagnostics.insert("diag:contradiction-1".into(), Diagnostic {
            rule: "contradiction".into(),
            severity: "error".into(),
            subjects: vec!["req:a".into(), "req:b".into()],
            message: "cannot both hold".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        });
        let ok = |a: Value| eval_check("diagnosticExists", &a, &s, 0);
        assert_eq!(ok(serde_json::json!({"rule": "contradiction"})), None);
        assert_eq!(ok(serde_json::json!({"rule": "contradiction", "subject": "req:a"})), None);
        assert_eq!(ok(serde_json::json!({"rule": "contradiction", "subjects": ["req:a", "req:b"]})), None);
        assert!(ok(serde_json::json!({"rule": "contradiction", "subjects": ["req:a", "req:c"]})).is_some());
        assert!(ok(serde_json::json!({"rule": "duplicate-requirement", "subjects": ["req:a", "req:b"]})).is_some());
    }

    #[test]
    fn results_file_updates_in_place_per_model() {
        let tmp = std::env::temp_dir().join(format!("jazyk-bench-results-{}", std::process::id()));
        std::fs::remove_dir_all(&tmp).ok();
        write_results(
            &tmp,
            "model-a",
            &[("native".into(), serde_json::json!({"verdict": "review"}))],
        );
        write_results(
            &tmp,
            "model-b",
            &[("text".into(), serde_json::json!({"verdict": "extraction"}))],
        );
        write_results(
            &tmp,
            "model-a",
            &[(
                "native".into(),
                serde_json::json!({"verdict": "extraction"}),
            )],
        );
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
        let converge = cases.iter().find(|c| c.name == "converge").unwrap();
        let tmp = std::env::temp_dir().join("jazyk-bench-test");
        let s = sandbox(converge, &tmp);
        assert!(s.graph.entities.contains_key("ent:cart"));
        assert!(s.graph.requirements.contains_key("req:shop-1"));
        assert_eq!(
            s.docs["docs/shop.md"].coverage["/shop/checkout"].state,
            "covered"
        );
        // The fixture's quote must locate in the parsed section, or the case is unwinnable.
        let r = &s.graph.requirements["req:shop-1"];
        assert!(s.quote_locates(
            &r.source.as_ref().unwrap().doc,
            &r.source.as_ref().unwrap().section,
            &r.source.as_ref().unwrap().quote
        ));
    }
}
