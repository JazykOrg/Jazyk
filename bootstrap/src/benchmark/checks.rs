use crate::llm;
use crate::model::*;
use crate::store::Store;
use crate::vfs::VPathExt;
use serde_json::Value;

use super::*;

// Check patterns are regular expressions per case.schema.yaml, matched
// case-insensitively. An invalid pattern is a check failure, never a silent pass.
pub(super) fn compile(pattern: &str) -> Result<regex::Regex, String> {
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
            let re = match compile(pat) {
                Ok(re) => re,
                Err(e) => return Some(e),
            };
            // The entity is optional: without one, any requirement whose statement
            // matches counts (docs/benchmark/cases.md#checks).
            let Some(ent) = arg["entity"].as_str() else {
                let found = store
                    .graph
                    .requirements
                    .values()
                    .any(|r| re.is_match(&r.statement));
                return (!found).then(|| format!("no requirement matching `{}`", pat));
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
        "criteriaMatching" => {
            let rx = match compile(arg["pattern"].as_str().unwrap_or_default()) {
                Ok(r) => r,
                Err(e) => return Some(e),
            };
            let n = store
                .graph
                .criteria
                .values()
                .filter(|c| match arg["requirement"].as_str() {
                    Some(rid) => store.resolve_id(&c.requirement) == store.resolve_id(rid),
                    None => true,
                })
                .filter(|c| rx.is_match(&c.statement))
                .count();
            within(n, arg, "matching criteria")
        }
        "attributeExists" => {
            let ent = arg["entity"].as_str().unwrap_or_default();
            let Some(ent_id) = find_entity(store, ent) else {
                return Some(format!("entity {} not found", ent));
            };
            let rx = match compile(arg["name"].as_str().unwrap_or_default()) {
                Ok(r) => r,
                Err(e) => return Some(e),
            };
            let names: Vec<String> = store
                .graph
                .entities
                .get(&ent_id)
                .map(|e| e.attributes.iter().map(|a| a.name.clone()).collect())
                .unwrap_or_default();
            if names.iter().any(|n| rx.is_match(n)) {
                None
            } else {
                Some(format!(
                    "{} has no attribute matching `{}`; its attributes: {}",
                    ent,
                    arg["name"].as_str().unwrap_or_default(),
                    if names.is_empty() {
                        "(none)".to_string()
                    } else {
                        names.join(", ")
                    }
                ))
            }
        }
        "criteriaCount" => {
            let rx = match arg["statementPattern"].as_str() {
                Some(p) => match compile(p) {
                    Ok(r) => Some(r),
                    Err(e) => return Some(e),
                },
                None => None,
            };
            let n = store
                .graph
                .criteria
                .values()
                .filter(|c| {
                    let owner = store.resolve_id(&c.requirement);
                    if let Some(rid) = arg["requirement"].as_str() {
                        if owner != store.resolve_id(rid) {
                            return false;
                        }
                    }
                    if let Some(r) = &rx {
                        return store
                            .graph
                            .requirements
                            .get(owner)
                            .map(|q| r.is_match(&q.statement))
                            .unwrap_or(false);
                    }
                    true
                })
                .count();
            within(n, arg, "criteria")
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
            // `type` names one accepted type, `types` any of several
            // (docs/benchmark/cases.md#checks).
            let mut want: Vec<String> = str_list(&arg["types"]);
            if let Some(t) = arg["type"].as_str() {
                want.push(t.to_string());
            }
            let found = store.graph.relationships.values().any(|r| {
                let members: Vec<String> = r
                    .members
                    .iter()
                    .map(|m| store.resolve_id(m).to_string())
                    .collect();
                members.contains(&a_id)
                    && members.contains(&b_id)
                    && (want.is_empty() || r.contributions.iter().any(|c| want.contains(&c.r#type)))
            });
            (!found).then(|| {
                format!(
                    "no {} relationship between {} and {}",
                    if want.is_empty() {
                        "derived".to_string()
                    } else {
                        want.join(" or ")
                    },
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
            // A diagnostic names a subject directly, or through a requirement whose
            // entities include it: a contradiction filed on two requirements of one
            // entity is on that entity. Mirrors docs/benchmark/cases.md#checks.
            let names =
                |d: &crate::model::Diagnostic, want: &str| {
                    let want = store.resolve_id(want);
                    d.subjects.iter().any(|s| {
                        let s = store.resolve_id(s);
                        s == want
                            || store.graph.requirements.get(s).is_some_and(|r| {
                                r.entities.iter().any(|e| store.resolve_id(e) == want)
                            })
                    })
                };
            let found = store.graph.diagnostics.values().any(|d| {
                d.lifecycle == "open" && d.rule == rule && wanted.iter().all(|want| names(d, want))
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
                let named: std::collections::BTreeSet<String> = from
                    .iter()
                    .map(|f| store.resolve_id(f).to_string())
                    .collect();
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
                            || store.graph.views.iter().any(|(oid, o)| {
                                oid != vid && o.members.iter().any(|x| resolve(x) == m)
                            });
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
        // Requirements carrying the facet, optionally only those whose statement
        // matches. Mirrors docs/benchmark/cases.md#checks.
        "facetCount" => {
            let facet = arg["facet"].as_str().unwrap_or_default();
            let rx = match arg["statementPattern"].as_str() {
                Some(p) => match compile(p) {
                    Ok(r) => Some(r),
                    Err(e) => return Some(e),
                },
                None => None,
            };
            let n = store
                .graph
                .requirements
                .values()
                .filter(|r| {
                    rx.as_ref()
                        .map(|re| re.is_match(&r.statement))
                        .unwrap_or(true)
                })
                .filter(|r| r.facets.iter().any(|f| f.facet == facet))
                .count();
            within(
                n,
                arg,
                &format!(
                    "`{}` requirements{}",
                    facet,
                    arg["statementPattern"]
                        .as_str()
                        .map(|p| format!(" matching `{}`", p))
                        .unwrap_or_default()
                ),
            )
        }
        // Requirements whose transition's subject is the entity; `min` defaults to 1.
        // Mirrors docs/benchmark/cases.md#checks.
        "transitionSubject" => {
            let ent = arg["entity"].as_str().unwrap_or_default();
            let Some(ent_id) = find_entity(store, ent) else {
                return Some(format!("entity {} not found", ent));
            };
            let subjects: Vec<String> = store
                .graph
                .requirements
                .values()
                .filter_map(|r| r.transition.as_ref())
                .map(|t| store.resolve_id(&t.subject).to_string())
                .collect();
            let n = subjects.iter().filter(|s| **s == ent_id).count();
            let mut bounds = arg.clone();
            if bounds["min"].is_null() {
                bounds["min"] = serde_json::json!(1);
            }
            within(n, &bounds, &format!("transitions on {}", ent_id)).map(|why| {
                let others: std::collections::BTreeSet<&String> =
                    subjects.iter().filter(|s| **s != ent_id).collect();
                if others.is_empty() {
                    why
                } else {
                    format!(
                        "{}; other subjects: {}",
                        why,
                        others.into_iter().cloned().collect::<Vec<_>>().join(", ")
                    )
                }
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
pub const WORKFLOW_CHECKS: [&str; 9] = [
    "generationRecorded",
    "rowPerRequirement",
    "testsPass",
    "testFalsifiable",
    "verdictIs",
    "statusIn",
    "testsFail",
    "fileMatches",
    "fileCount",
];

// Every file under the deliverable as a deliverable-relative path with `/` separators,
// sorted, so a path pattern reads the same on every platform.
fn deliverable_files(root: &std::path::Path) -> Vec<String> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<String>) {
        let Ok(rd) = crate::vfs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if e.is_dir() {
                walk(&p, root, out);
            } else if let Ok(rel) = p.strip_prefix(root) {
                let rel: Vec<String> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
                    .collect();
                out.push(rel.join("/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

// A count against the check's optional `min` and `max`, worded for the failure.
fn within_bounds(arg: &Value, n: usize, what: &str) -> Option<String> {
    let min = arg["min"].as_u64().map(|m| m as usize);
    let max = arg["max"].as_u64().map(|m| m as usize);
    if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
        let bounds = match (min, max) {
            (Some(a), Some(b)) => format!("{}..={}", a, b),
            (Some(a), None) => format!("at least {}", a),
            (None, Some(b)) => format!("at most {}", b),
            (None, None) => "any".into(),
        };
        return Some(format!("{} {}, expected {}", n, what, bounds));
    }
    None
}

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
            let slug = crate::ids::entity_slug(target).to_string();
            match ledger.entities.get(&slug) {
                None => Some("record_generation never landed".into()),
                Some(e) if e.files.is_empty() => Some("the manifest recorded no files".into()),
                Some(e) => {
                    for f in &e.files {
                        if !gs.deliverable.join(f).vexists() {
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
                return Some(include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/benchmark-checks/no-programmatic-test-rows-to-run-8335e.md")).into());
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
            let slug = crate::ids::entity_slug(target).to_string();
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
                if let Ok(text) = crate::vfs::read_to_string(&p) {
                    if text.contains(needle) {
                        crate::vfs::write(&p, text.replace(needle, "BROKEN")).ok();
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
                crate::vfs::write(p, text).ok();
            }
            match verdict {
                Ok(r) if r.pass => Some(format!(
                    "{}'s test still passes with the product broken; not falsifiable",
                    rid
                )),
                Ok(r) if r.runner_failed() => Some(format!(
                    "broken-product rerun did not complete: {}",
                    r.evidence
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
        // The row's derived status is one of the listed, whatever verdict the session
        // claimed. Mirrors docs/benchmark/cases.md#checks.
        "statusIn" => {
            let rid = arg["requirement"].as_str().unwrap_or_default();
            let want = str_list(&arg["statuses"]);
            let Some(row) = ledger.requirements.get(store.resolve_id(rid)) else {
                return Some(format!("no ledger row for {}", rid));
            };
            let (status, reason) = crate::verify::status_of(store, rid, row, gs);
            (!want.contains(&status)).then(|| {
                format!(
                    "{} reads `{}` ({}), expected one of {}",
                    rid,
                    status,
                    reason,
                    want.join(", ")
                )
            })
        }
        // Every programmatic test the row records fails when rerun as recorded: a
        // test that passes against a deliverable that carries nothing reads a
        // description. Mirrors docs/benchmark/cases.md#checks.
        "testsFail" => {
            let rid = arg["requirement"].as_str().unwrap_or_default();
            let Some(row) = ledger.requirements.get(store.resolve_id(rid)) else {
                return Some(format!("no ledger row for {}", rid));
            };
            let mut tests: Vec<&crate::gen::TestRef> =
                row.tests.values().map(|t| &t.test).collect();
            if tests.is_empty() {
                tests.push(&row.test);
            }
            let tests: Vec<&crate::gen::TestRef> = tests
                .into_iter()
                .filter(|t| t.kind == "programmatic")
                .collect();
            if tests.is_empty() {
                return Some(format!("{} records no programmatic test", rid));
            }
            for t in tests {
                if t.run.trim().is_empty() {
                    return Some(format!(include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/benchmark-checks/test-has-no-run-command-dad4b.md")), t.name));
                }
                match crate::verify::run_test_ref(gs, t) {
                    Ok(out) if out.pass => {
                        return Some(format!(
                            "test `{}` passes as recorded (`{}`); the case expects it to fail",
                            t.name, t.run
                        ))
                    }
                    Ok(out) if out.runner_failed() => {
                        return Some(format!(
                            "test `{}` did not complete: {}",
                            t.name, out.evidence
                        ))
                    }
                    Ok(_) => {}
                    Err(e) => return Some(format!(include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/benchmark-checks/test-did-not-run-cc1f7.md")), t.name, e)),
                }
            }
            None
        }
        // The conventions a generated deliverable follows are graded over its files,
        // since every test the model writes can be green while the manifest says
        // `latest`. Mirrors docs/benchmark/cases.md#checks.
        "fileMatches" => {
            let path = arg["path"].as_str().unwrap_or_default();
            let re = match compile(arg["pattern"].as_str().unwrap_or_default()) {
                Ok(re) => re,
                Err(e) => return Some(e),
            };
            let Ok(text) = crate::vfs::read_to_string(gs.deliverable.join(path)) else {
                return Some(format!("deliverable file `{}` does not exist", path));
            };
            let n = re.find_iter(&text).count();
            within_bounds(arg, n, &format!("matches of the pattern in `{}`", path))
        }
        "fileCount" => {
            let re = match compile(arg["pathPattern"].as_str().unwrap_or_default()) {
                Ok(re) => re,
                Err(e) => return Some(e),
            };
            let exclude = match arg["excludePattern"].as_str() {
                Some(p) => match compile(p) {
                    Ok(re) => Some(re),
                    Err(e) => return Some(e),
                },
                None => None,
            };
            let files: Vec<String> = deliverable_files(&gs.deliverable)
                .into_iter()
                .filter(|f| re.is_match(f) && !exclude.as_ref().is_some_and(|x| x.is_match(f)))
                .collect();
            within_bounds(
                arg,
                files.len(),
                &format!("files match ({})", files.join(", ")),
            )
        }
        other => Some(format!("unknown workflow check `{}`", other)),
    }
}
