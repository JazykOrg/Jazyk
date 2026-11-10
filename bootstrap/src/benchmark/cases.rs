use crate::model::*;
use crate::project::Linting;
use serde_json::Value;
use std::collections::BTreeMap;

const CASE_FILES: [&str; 32] = [
    include_str!("../../../docs/benchmark/cases/criteria-observation.md"),
    include_str!("../../../docs/benchmark/cases/enumeration.md"),
    include_str!("../../../docs/benchmark/cases/paragraph-layout.md"),
    include_str!("../../../docs/benchmark/cases/properties.md"),
    include_str!("../../../docs/benchmark/cases/criteria-density.md"),
    include_str!("../../../docs/benchmark/cases/extract.md"),
    include_str!("../../../docs/benchmark/cases/navigation.md"),
    include_str!("../../../docs/benchmark/cases/declarative.md"),
    include_str!("../../../docs/benchmark/cases/density.md"),
    include_str!("../../../docs/benchmark/cases/edges.md"),
    include_str!("../../../docs/benchmark/cases/reuse.md"),
    include_str!("../../../docs/benchmark/cases/converge.md"),
    include_str!("../../../docs/benchmark/cases/repair.md"),
    include_str!("../../../docs/benchmark/cases/review.md"),
    include_str!("../../../docs/benchmark/cases/review-duplicate.md"),
    include_str!("../../../docs/benchmark/cases/review-lookalike.md"),
    include_str!("../../../docs/benchmark/cases/review-lint.md"),
    include_str!("../../../docs/benchmark/cases/rejudge-pair.md"),
    include_str!("../../../docs/benchmark/cases/dedupe-candidates.md"),
    include_str!("../../../docs/benchmark/cases/abstract-entity.md"),
    include_str!("../../../docs/benchmark/cases/split-view.md"),
    include_str!("../../../docs/benchmark/cases/curate-view.md"),
    include_str!("../../../docs/benchmark/cases/declare-edges.md"),
    include_str!("../../../docs/benchmark/cases/steps.md"),
    include_str!("../../../docs/benchmark/cases/narrative-behavior.md"),
    include_str!("../../../docs/benchmark/cases/machine-subject.md"),
    include_str!("../../../docs/benchmark/cases/stated-containment.md"),
    include_str!("../../../docs/benchmark/cases/gen-basic.md"),
    include_str!("../../../docs/benchmark/cases/gen-node.md"),
    include_str!("../../../docs/benchmark/cases/bind-descriptor.md"),
    include_str!("../../../docs/benchmark/cases/tech-constraint.md"),
    include_str!("../../../docs/benchmark/cases/verify-judge.md"),
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
    // Criteria seeded on the fixture's requirements, for the ledger-tier kinds.
    pub criteria: BTreeMap<String, Value>,
    pub coverage: BTreeMap<String, String>,
    pub lint: Linting,
    pub checks: Vec<(String, Value)>,
    // Replays only: the scripted calls. Mirrors docs/benchmark/replays.md#replay-format.
    pub steps: Vec<Step>,
    // Replays only: the batch's other goals as (kind, target).
    pub extra_goals: Vec<(String, String)>,
}

// One scripted call of a replay and the reply the harness must give.
// Mirrors docs/benchmark/replays.md#replay-format.
#[derive(Clone, Debug)]
pub struct Step {
    pub call: String,
    pub args: Value,
    pub ok: Option<bool>,
    pub rule: Option<String>,
    pub contains: Option<String>,
    pub absent: Option<String>,
}

// The harness task name a goal kind assembles to, for the kinds whose goal is
// assembled from kind and target rather than derived (docs/benchmark/cases.md#derived-goals).
pub fn assembled_task(kind: &str) -> Option<&'static str> {
    let task: &'static str = match kind {
        "reconcile-section" => "reconcile-doc",
        "generate" => "generate-entity",
        "verify" => "verify-requirement",
        "review-entity" => "review-entity",
        _ => return None,
    };
    ASSEMBLED_TASKS.contains(&task).then_some(task)
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
pub fn yaml_blocks(md: &str) -> Vec<String> {
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

// One case from its yaml value. Mirrors docs/benchmark/cases.md#case-format.
pub fn case_from_value(v: &Value) -> Case {
    {
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
        Case {
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
            criteria: obj(&v["given"]["graph"]["criteria"]),
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
            extra_goals: v["goals"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|g| {
                            Some((
                                g["kind"].as_str()?.to_string(),
                                g["target"].as_str()?.to_string(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            steps: v["steps"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|st| {
                            let call = st["call"].as_str()?.to_string();
                            let e = &st["expect"];
                            Some(Step {
                                call,
                                args: if st["args"].is_null() {
                                    serde_json::json!({})
                                } else {
                                    st["args"].clone()
                                },
                                ok: e["ok"].as_bool(),
                                rule: e["rule"].as_str().map(String::from),
                                contains: e["contains"].as_str().map(String::from),
                                absent: e["absent"].as_str().map(String::from),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
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
            cases.push(case_from_value(&v));
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

pub(super) fn str_list(v: &Value) -> Vec<String> {
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
pub(super) fn entity_of(e: &Value) -> Entity {
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
pub(super) fn requirement_of(r: &Value) -> Option<Requirement> {
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

// A fixture criterion, in the criterion's own serialized shape: requirement,
// statement, kind, and a `source` quote or a `provenance`.
pub(super) fn criterion_of(c: &Value) -> Result<Criterion, String> {
    let mut fields = c.clone();
    if let Some(m) = fields.as_object_mut() {
        m.remove("source");
    }
    let mut crit: Criterion = serde_json::from_value(fields).map_err(|e| e.to_string())?;
    crit.source = source_ref(&c["source"]);
    Ok(crit)
}

// A fixture view, in the view's own serialized shape. A seeded view is curated.
pub(super) fn view_of(v: &Value) -> Result<View, String> {
    let mut view: View = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    view.default = false;
    Ok(view)
}
