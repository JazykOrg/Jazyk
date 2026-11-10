use super::*;

// ---- review-entity ----

pub struct ReviewEntity;

// Lookalikes of an entity from the name index: name-similar (a token superset either
// way, or a shared stem) and related but separate (a name that extends this one).
pub fn lookalikes(store: &Store, id: &str) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let Some(e) = store.graph.entities.get(id) else {
        return (Vec::new(), Vec::new());
    };
    let mine = name_tokens(e);
    let mut similar = Vec::new();
    let mut related = Vec::new();
    for (oid, o) in &store.graph.entities {
        if oid == id || o.scope != e.scope {
            continue;
        }
        let theirs = name_tokens(o);
        if mine.is_empty() || theirs.is_empty() {
            continue;
        }
        let shared = mine.intersection(&theirs).count();
        if shared == 0 {
            continue;
        }
        let dice = 2.0 * shared as f64 / (mine.len() + theirs.len()) as f64;
        if dice >= 0.66 {
            similar.push((oid.clone(), o.name.clone()));
        } else if mine.is_subset(&theirs) || theirs.is_subset(&mine) {
            related.push((oid.clone(), o.name.clone()));
        }
    }
    similar.truncate(6);
    related.truncate(6);
    (similar, related)
}

// Statements whose prose names the entity or an alias (word-bounded, code spans
// blanked) without referencing it.
pub fn unreferenced_statements(store: &Store, id: &str) -> Vec<(String, String)> {
    let Some(e) = store.graph.entities.get(id) else {
        return Vec::new();
    };
    let names: Vec<String> = std::iter::once(&e.name)
        .chain(e.aliases.iter())
        .map(|n| n.to_lowercase())
        .filter(|n| n.len() >= 3)
        .collect();
    let blank_code = |s: &str| -> String {
        let mut out = String::new();
        let mut inside = false;
        for c in s.chars() {
            if c == '`' {
                inside = !inside;
                out.push(' ');
            } else if inside {
                out.push(' ');
            } else {
                out.push(c);
            }
        }
        out.to_lowercase()
    };
    let word_bounded = |hay: &str, needle: &str| -> bool {
        let mut start = 0;
        while let Some(pos) = hay[start..].find(needle) {
            let b = start + pos;
            let e = b + needle.len();
            let before_ok = b == 0 || !hay[..b].chars().last().unwrap().is_alphanumeric();
            let after_ok = e == hay.len() || !hay[e..].chars().next().unwrap().is_alphanumeric();
            if before_ok && after_ok {
                return true;
            }
            start = e;
        }
        false
    };
    let mut out = Vec::new();
    for (rid, r) in &store.graph.requirements {
        // A word match a review already judged stays off the list
        // (docs/compiler/model/entity.md#fields).
        if e.not_about.contains(rid) || r.entities.iter().any(|x| store.resolve_id(x) == id) {
            continue;
        }
        // The quote is the document's own text; a statement is the session's wording,
        // and a paraphrase that added the name would otherwise raise its own hint
        // (docs/compiler/goals/review-entity.md#hints).
        // Both the quote (the document's text) and the statement (the session's) must
        // name the entity: a paraphrase that added the name fails the first, a sentence
        // split into several statements fails the second for the parts not about it.
        let quote_text = blank_code(
            r.source
                .as_ref()
                .map(|s| s.quote.as_str())
                .unwrap_or(&r.statement),
        );
        let statement_text = blank_code(&r.statement);
        if names
            .iter()
            .any(|n| word_bounded(&quote_text, n) && word_bounded(&statement_text, n))
        {
            out.push((rid.clone(), r.statement.clone()));
            if out.len() >= 6 {
                break;
            }
        }
    }
    out
}

// The composition edges on an entity's statements whose part has no parent, or a
// parent outside the stated whole: (whole, part, hint line), one per edge. The
// review hint lists them and the review gate holds one `parts` verdict per part.
// Mirrors docs/compiler/goals/review-entity.md#hints and #gate.
pub fn unparented_parts(store: &Store, id: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    for r in store.requirements_referencing(id) {
        let Some(q) = store.graph.requirements.get(&r) else {
            continue;
        };
        for edge in &q.edges {
            if edge.rel_type.as_deref() != Some("composition") {
                continue;
            }
            let Some(part) = store.graph.entities.get(&edge.b) else {
                continue;
            };
            // A parent under the stated whole satisfies the composition
            // transitively (a scene under a chapter under the manuscript).
            let line = match &part.parent {
                None => format!("{} -> {} (whole -> part)", edge.a, edge.b),
                Some(p) if p != &edge.a && !store.is_ancestor(&edge.a, &edge.b) => {
                    format!(
                        "{} -> {} (whole -> part; parent now {}, not under the whole)",
                        edge.a, edge.b, p
                    )
                }
                _ => continue,
            };
            if !out.iter().any(|(_, _, l)| l == &line) {
                out.push((edge.a.clone(), edge.b.clone(), line));
            }
        }
    }
    out
}

// The open diagnostics a review lists on an entity and its statements, harness-owned
// rules aside: the review gate holds one `diagnostics` verdict per id.
// Mirrors docs/compiler/goals/review-entity.md#gate.
pub fn review_diagnostics(store: &Store, id: &str) -> Vec<String> {
    let reqs = store.requirements_referencing(id);
    let mut subjects: Vec<&str> = reqs.iter().map(String::as_str).collect();
    subjects.push(id);
    open_diags_naming(store, &subjects)
        .into_iter()
        .map(|(did, _)| did.clone())
        .collect()
}

impl GoalKind for ReviewEntity {
    fn kind(&self) -> &'static str {
        "review-entity"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "entity"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut by_subject: BTreeMap<String, Vec<&ChangeRecord>> = BTreeMap::new();
        for rec in records_of(store, &[store::CHANGE_ENTITY, store::CHANGE_NODE_DELETED]) {
            if !rec.subject.starts_with("ent:") || !store.graph.entities.contains_key(&rec.subject)
            {
                continue;
            }
            if rec.kind == store::CHANGE_NODE_DELETED && rec.via != "subjects" {
                continue;
            }
            by_subject.entry(rec.subject.clone()).or_default().push(rec);
        }
        let mut out = Vec::new();
        for (id, recs) in by_subject {
            let e = &store.graph.entities[&id];
            let vias: BTreeSet<String> = recs.iter().map(|r| r.via.clone()).collect();
            let mut requirements: Vec<String> = Vec::new();
            let mut deleted: Vec<String> = Vec::new();
            for r in &recs {
                requirements.extend(str_list(&r.detail["requirements"]));
                deleted.extend(str_list(&r.detail["deleted"]));
            }
            requirements.sort();
            requirements.dedup();
            let change = json!({
                "via": vias,
                "requirements": requirements,
                "deleted": deleted,
            });
            let cause = earliest_cause(&recs);
            let reqs = store.requirements_referencing(&id);
            let mut hints = vec![format!(
                "entity-changed via {} in g{}; {} requirement(s) on it",
                vias.iter().cloned().collect::<Vec<_>>().join(", "),
                cause.as_ref().map(|c| c.generation).unwrap_or(0),
                reqs.len()
            )];
            let (similar, related) = lookalikes(store, &id);
            if !similar.is_empty() {
                hints.push(format!(
                    "lookalikes (name-similar, a merge when one concept): {}",
                    similar
                        .iter()
                        .map(|(i, n)| format!("{} ({})", i, n))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !related.is_empty() {
                hints.push(format!(
                    "lookalikes (related, separate by default): {}",
                    related
                        .iter()
                        .map(|(i, n)| format!("{} ({})", i, n))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            let unref = unreferenced_statements(store, &id);
            if !unref.is_empty() {
                hints.push(format!(
                    "unreferenced (word matches, not judgments): {}",
                    unref
                        .iter()
                        .map(|(r, s)| format!("{} \"{}\"", r, truncate(s, 60)))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            let no_edges: Vec<&String> = reqs
                .iter()
                .filter(|r| {
                    store
                        .graph
                        .requirements
                        .get(*r)
                        .is_some_and(|q| q.entities.len() >= 2 && q.edges.is_empty())
                })
                .collect();
            if !no_edges.is_empty() {
                hints.push(format!(
                    "multi-entity statements without edges (each has its own declare-edges goal; leave the edges to it): {}",
                    no_edges
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            // A stated composition whose part has no parent, or a parent disagreeing
            // with the stated whole: the review's contract says to set parent on the
            // part, so name each such edge (both the missing and the mismatch case).
            let parentless: Vec<String> = unparented_parts(store, &id)
                .into_iter()
                .map(|(_, _, line)| line)
                .collect();
            if !parentless.is_empty() {
                hints.push(format!(
                    "composition edges whose part has no parent or a parent outside the stated whole (the edge reads whole -> part; set parent on the part with update_entity, or fix a reversed edge on the requirement, unless the part is a generic rule-carrier such as Department beside the named departments, which the judgment skill keeps parentless): {}",
                    parentless.join(", ")
                ));
            }
            let mut subjects: Vec<&str> = reqs.iter().map(String::as_str).collect();
            subjects.push(&id);
            let diags = open_diags_naming(store, &subjects);
            if !diags.is_empty() {
                hints.push(format!(
                    "{} open diagnostic(s): {}",
                    diags.len(),
                    diags
                        .iter()
                        .map(|(did, d)| {
                            let dead: Vec<&str> = d
                                .subjects
                                .iter()
                                .filter(|s| {
                                    !store.graph.requirements.contains_key(*s)
                                        && !store.graph.entities.contains_key(*s)
                                })
                                .map(String::as_str)
                                .collect();
                            format!(
                                "{} ({}{})",
                                did,
                                d.rule,
                                if dead.is_empty() {
                                    String::new()
                                } else {
                                    format!(", deleted: {}", dead.join(", "))
                                }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            // The level views around the entity, so a review never guesses a view
            // id. Mirrors docs/compiler/goals/review-entity.md#hints.
            {
                let mut views = Vec::new();
                if let Some(v) = crate::derive::level_view_id(store, &id) {
                    views.push(format!("{} (this entity)", v));
                }
                let level_target = match &e.parent {
                    Some(p) => store.resolve_id(p).to_string(),
                    None => format!("{}{}", crate::board::SCOPE_TARGET_PREFIX, e.scope),
                };
                if let Some(v) = crate::derive::level_view_id(store, &level_target) {
                    views.push(format!("{} (the level above)", v));
                }
                if !views.is_empty() {
                    hints.push(format!("level views: {}", views.join(", ")));
                }
            }
            if let Some((soft, _)) = limits::threshold(
                "requirements-per-entity",
                e.limits.get("requirements-per-entity").map(|b| b.value),
            ) {
                if reqs.len() as u64 > soft {
                    hints.push(format!(
                        "{} requirements > {} (requirements-per-entity); abstract-entity follows, information only",
                        reqs.len(),
                        soft
                    ));
                }
            }
            hints.push(format!("load {}", id));
            hints.push("skill judgment; update_entity, merge_entities, update_requirement, report_diagnostic, resolve_diagnostic".into());
            out.push(build_goal(self, &id, true, change, cause, hints));
        }
        out
    }
    fn ready(&self, _goal: &Goal, board: &Board) -> Ready {
        tier2_ready(board)
    }
    fn pack(&self, store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        for g in batch {
            lines.push(format!("- {} full", g.target));
            let (similar, related) = lookalikes(store, &g.target);
            for (id, _) in similar.iter().chain(related.iter()) {
                lines.push(format!("- {} stub", id));
            }
            for (rid, _) in unreferenced_statements(store, &g.target) {
                lines.push(format!("- {} stub", rid));
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "update_entity",
            "merge_entities",
            "update_requirement",
            "delete_requirement",
            "report_diagnostic",
            "resolve_diagnostic",
        ]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = RejudgePair.gates(store, staged);
        for op in staged {
            if let Op::MergeEntities { keep, absorb, .. } = op {
                let scope = |id: &str| store.graph.entities.get(id).map(|e| e.scope.clone());
                if let (Some(a), Some(b)) = (scope(keep), scope(absorb)) {
                    if a != b {
                        out.push(Violation::new(
                            "scope-mismatch",
                            format!(
                                "merge_entities {} into {} crosses scopes ({} and {})",
                                absorb, keep, b, a
                            ),
                        ));
                    }
                }
            }
        }
        out
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../../docs/compiler/goals/prompts/review-entity.md")
    }
}
