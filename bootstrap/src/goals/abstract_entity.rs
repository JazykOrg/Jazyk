use super::*;

pub struct AbstractEntity;

// Candidate cohesion groups: content tokens recurring across at least three of the
// entity's statements, function words dropped, each shown as the word the statements
// use most (never a stem), with the section quoting most of them.
// Mirrors docs/compiler/goals/abstract-entity.md#hints.
pub fn cohesion_groups(store: &Store, id: &str) -> Vec<(String, usize, String)> {
    // Function words the pair scorer's stop list keeps (they carry weight between two
    // statements) that name no concept a group could be about.
    const FUNCTION: [&str; 72] = [
        "every", "from", "must", "may", "can", "will", "does", "did", "has", "have", "had", "into",
        "than", "more", "most", "one", "any", "some", "their", "they", "them", "after", "before",
        "about", "over", "under", "other", "same", "also", "via", "whenever", "while", "once",
        "per", "what", "who", "where", "how", "never", "always", "between", "within", "without",
        "through", "until", "since", "both", "either", "first", "last", "new", "such", "these",
        "those", "there", "here", "done", "make", "made", "use", "used", "like", "unless",
        "because", "would", "should", "could", "being", "been", "was", "were", "whose",
    ];
    let reqs = store.requirements_referencing(id);
    let mut by_token: BTreeMap<String, Vec<&String>> = BTreeMap::new();
    let mut words: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for rid in &reqs {
        let r = &store.graph.requirements[rid];
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for (stemmed, word) in content_token_words(store, &r.statement, &r.entities) {
            if stemmed.len() < 4
                || FUNCTION.contains(&stemmed.as_str())
                || FUNCTION.contains(&word.as_str())
            {
                continue;
            }
            *words
                .entry(stemmed.clone())
                .or_default()
                .entry(word)
                .or_insert(0) += 1;
            if seen.insert(stemmed.clone()) {
                by_token.entry(stemmed).or_default().push(rid);
            }
        }
    }
    let mut groups: Vec<(String, usize, String)> = by_token
        .into_iter()
        .filter(|(_, rids)| rids.len() >= 3)
        .map(|(tok, rids)| {
            let mut sections: BTreeMap<String, usize> = BTreeMap::new();
            for rid in &rids {
                if let Some(s) = store.graph.requirements[*rid].source.as_ref() {
                    *sections.entry(s.section.clone()).or_insert(0) += 1;
                }
            }
            let section = sections
                .into_iter()
                .max_by_key(|(s, n)| (*n, std::cmp::Reverse(s.clone())))
                .map(|(s, _)| s)
                .unwrap_or_default();
            let word = words
                .get(&tok)
                .and_then(|w| {
                    w.iter()
                        .max_by_key(|(word, n)| (**n, std::cmp::Reverse((*word).clone())))
                        .map(|(word, _)| word.clone())
                })
                .unwrap_or(tok);
            (word, rids.len(), section)
        })
        .collect();
    groups.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    groups.truncate(6);
    groups
}

// The section that defines an entity, the ratification proposal's target: the root
// section of a document headed with the entity's name, else the section among its
// mentions quoting the most of its requirements, else its first mention (a front
// door's list item names the entity without defining it). Mirrors
// docs/compiler/goals/abstract-entity.md#hints.
pub fn defining_section(store: &Store, id: &str) -> Option<(String, String)> {
    let e = store.graph.entities.get(id)?;
    let mut docs: Vec<&String> = e.mentions.iter().map(|m| &m.doc).collect();
    docs.dedup();
    for doc in docs {
        let root = store.docs.get(doc).and_then(|d| {
            d.sections
                .iter()
                .find(|(_, s)| s.kind == "root")
                .map(|(path, s)| (path.clone(), s.title.clone()))
        });
        if let Some((path, title)) = root {
            if title.trim().eq_ignore_ascii_case(e.name.trim()) {
                return Some((doc.clone(), path));
            }
        }
    }
    let mut quoting: BTreeMap<(String, String), usize> = BTreeMap::new();
    for rid in store.requirements_referencing(id) {
        if let Some(s) = &store.graph.requirements[&rid].source {
            *quoting
                .entry((s.doc.clone(), s.section.clone()))
                .or_insert(0) += 1;
        }
    }
    // Among the mention sections, the one quoting most; ties keep mention order.
    e.mentions
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let key = (m.doc.clone(), m.section.clone());
            let n = quoting.get(&key).copied().unwrap_or(0);
            (n, std::cmp::Reverse(i), key)
        })
        .max()
        .map(|(_, _, key)| key)
}

// The caps gate for one goal at mark_goal_done: the requirement count on the target
// after the changeset is within the soft value in force (the record folds the
// entity's own bump in). A split that only de-escalates is refused here, so the
// session moves more detail or fails the goal with the reason. None when no
// requirement record stands on the target. Mirrors
// docs/compiler/goals/abstract-entity.md#gate.
pub fn caps_goal_gate(store: &Store, staged: &[Op], target: &str) -> Option<Violation> {
    let rec = store.status.changes.iter().find(|c| {
        c.kind == store::CHANGE_THRESHOLD_CROSSED
            && c.subject == target
            && c.detail["limit"] == "requirements-per-entity"
    })?;
    let soft = rec.detail["soft"].as_u64()?;
    let hard = rec.detail["hard"].as_u64().unwrap_or(soft);
    // The count is the statements the target leads (first-listed entity), as the
    // threshold counts them (docs/compiler/goals/abstract-entity.md#created-when).
    let names_target = |entities: &[String]| {
        entities
            .first()
            .is_some_and(|x| store.resolve_id(x) == target)
    };
    let mut referencing: BTreeSet<String> = store
        .requirements_referencing(target)
        .into_iter()
        .filter(|id| {
            store
                .graph
                .requirements
                .get(id)
                .is_some_and(|r| names_target(&r.entities))
        })
        .collect();
    for op in staged {
        match op {
            Op::UpdateRequirement {
                id,
                entities: Some(e),
                ..
            } => {
                if names_target(e) {
                    referencing.insert(id.clone());
                } else {
                    referencing.remove(id);
                }
            }
            Op::CreateRequirement { id, requirement } => {
                if names_target(&requirement.entities) {
                    referencing.insert(id.clone());
                }
            }
            Op::DeleteRequirement { id, .. } => {
                referencing.remove(id);
            }
            _ => {}
        }
    }
    let count = referencing.len() as u64;
    if count <= soft {
        return None;
    }
    Some(Violation::new(
        "count-over-limit",
        format!(
            "{}: {} requirements after the changeset, over {} (requirements-per-entity, soft {}, hard {}); two honest exits, and the staged split commits at done either way: re-point more grouped statements to the sub-entities with update_requirement (id and entities only) when a documented noun holds them, or mark_goal_failed now with the reason that the remaining statements are about the whole, recommending a bump of the entity's own limit to the count",
            target, count, soft, soft, hard
        ),
    ))
}

impl GoalKind for AbstractEntity {
    fn kind(&self) -> &'static str {
        "abstract-entity"
    }
    fn class(&self) -> Class {
        Class::Gc
    }
    fn unit(&self) -> &'static str {
        "entity"
    }
    // The caps variant on an entity's requirement and state counts, the fan-out
    // variant on a level's child count (a node, or `scope:<scope>` for the top level);
    // records for both on one node fold into one goal. Mirrors
    // docs/compiler/goals/abstract-entity.md#created-when and #the-fan-out-variant.
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut by_target: BTreeMap<String, Vec<&ChangeRecord>> = BTreeMap::new();
        for rec in records_of(store, &[store::CHANGE_THRESHOLD_CROSSED]) {
            let mine = rec.detail["goal"] == "abstract-entity"
                || rec.detail["limit"] == limits::CHILDREN_PER_ENTITY;
            let subject_exists = store.graph.entities.contains_key(&rec.subject)
                || scope_target(&rec.subject).is_some();
            if mine && subject_exists {
                by_target.entry(rec.subject.clone()).or_default().push(rec);
            }
        }
        let mut out = Vec::new();
        for (id, recs) in by_target {
            let crossed = crossings_for(&recs);
            let mandatory = crossed.iter().any(|c| c.count > c.hard);
            let cause = earliest_cause(&recs);
            let (fan, caps): (Vec<&Crossed>, Vec<&Crossed>) = crossed
                .iter()
                .partition(|c| c.limit == limits::CHILDREN_PER_ENTITY);
            let caps: Vec<Crossed> = caps
                .into_iter()
                .map(|c| Crossed {
                    limit: c.limit.clone(),
                    count: c.count,
                    soft: c.soft,
                    hard: c.hard,
                })
                .collect();
            let mut hints = vec![format!("load {}", id)];
            let mut change = match fan.first() {
                Some(c) => {
                    let f = FanOut {
                        count: c.count,
                        soft: c.soft,
                        hard: c.hard,
                    };
                    let cands = coupling_candidates(store, &id, f.soft);
                    hints.extend(fan_out_hints(store, &id, &f, &cands));
                    json!({
                        "fan_out": f.count,
                        "limit": {"soft": f.soft, "hard": f.hard},
                        "candidates": cands.iter().map(|c| c.ids.clone()).collect::<Vec<_>>(),
                    })
                }
                None => Value::Null,
            };
            if !caps.is_empty() {
                let caps_change = threshold_change(&caps);
                match change.as_object_mut() {
                    Some(o) => {
                        o.insert("limits".into(), caps_change["limits"].clone());
                    }
                    None => change = caps_change,
                }
            }
            if let Some(e) = store.graph.entities.get(&id).filter(|_| !caps.is_empty()) {
                for c in &caps {
                    hints.push(format!(
                        "{} > {} ({}, soft {}, hard {})",
                        c.count, c.soft, c.limit, c.soft, c.hard
                    ));
                    if c.limit == "states-per-state-machine" {
                        hints.push(format!("load sm:{}", crate::ids::entity_slug(&id)));
                    }
                }
                for (tok, n, section) in cohesion_groups(store, &id) {
                    hints.push(format!("group {} ({} requirements, {})", tok, n, section));
                }
                for cid in level_members(store, &id) {
                    hints.push(format!(
                        "child {} ({} requirements)",
                        cid,
                        store.requirements_referencing(&cid).len()
                    ));
                }
                if let Some((doc, section)) = defining_section(store, &id) {
                    let full = format!("{}#{}", doc, section);
                    let too_large = store.graph.diagnostics.values().any(|d| {
                        d.lifecycle == "open"
                            && d.rule == "section-too-large"
                            && d.subjects.contains(&full)
                    });
                    hints.push(format!(
                        "load {}{}",
                        full,
                        if too_large {
                            " (section-too-large filed)"
                        } else {
                            ""
                        }
                    ));
                }
                let composed = store
                    .graph
                    .relationships
                    .values()
                    .flat_map(|r| r.contributions.iter())
                    .any(|c| c.r#type == "composition" && c.a == id);
                if e.parent.is_none() && !composed {
                    hints.push(
                        "root: no parent, no composition stated; a decision prompt is owed".into(),
                    );
                }
            }
            let tools = match (fan.is_empty(), caps.is_empty()) {
                (false, true) => "skill abstraction; group_entities, update_entity, dissolve_entity",
                (false, false) => "skill abstraction; group_entities, update_entity, dissolve_entity; upsert_entity, update_requirement",
                _ => "skill abstraction; upsert_entity, update_requirement, update_entity",
            };
            hints.push(tools.into());
            out.push(build_goal(self, &id, mandatory, change, cause, hints));
        }
        out
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        gc_ready(goal, board)
    }
    // The node (or `scope:<scope>`, the top level as stubs) full and its children as
    // stubs. The caps variant adds the defining section; the fan-out variant the
    // level's views and the existing groupings' children. Mirrors
    // docs/compiler/goals/abstract-entity.md#what-the-model-sees and #the-fan-out-prompt.
    fn pack(&self, store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        for g in batch {
            let fan_out = g.change.get("fan_out").is_some();
            let caps = !fan_out || g.change.get("limits").is_some();
            lines.push(format!("- {} full", g.target));
            let members = level_members(store, &g.target);
            for cid in &members {
                lines.push(format!("- {} stub", cid));
            }
            if caps {
                if let Some((doc, section)) = defining_section(store, &g.target) {
                    lines.push(format!("- {}#{} full", doc, section));
                }
            }
            if fan_out {
                for m in &members {
                    for cid in level_members(store, m) {
                        lines.push(format!("- {} stub", cid));
                    }
                }
            }
            for (vid, v) in &store.graph.views {
                let level_view = v
                    .query
                    .as_ref()
                    .and_then(|q| q.parent.as_deref())
                    .is_some_and(|p| p == g.target)
                    || scope_target(&g.target).is_some_and(|s| {
                        vid == &format!("view:component/scope-{}", s)
                            || vid == &format!("view:class/scope-{}", s)
                    });
                if v.members.contains(&g.target) || level_view {
                    lines.push(format!("- {} stub", vid));
                }
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "upsert_entity",
            "update_entity",
            "upsert_requirement",
            "update_requirement",
            "group_entities",
            "dissolve_entity",
            "upsert_view",
            "update_view",
            "report_diagnostic",
        ]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = fan_out_gates(store, staged);
        let staged_parents: BTreeSet<String> = staged
            .iter()
            .filter_map(|o| match o {
                Op::CreateEntity { id, entity } if entity.parent.is_some() => Some(id.clone()),
                _ => None,
            })
            .collect();
        for op in staged {
            match op {
                Op::CreateEntity { id, entity } if entity.parent.is_some() => {
                    match &entity.provenance {
                        Some(Provenance::Derived { from, reasoning }) => {
                            if from.is_empty() || reasoning.trim().is_empty() {
                                out.push(Violation::new(
                                    "provenance-required",
                                    format!("{}: derived provenance needs from and reasoning", id),
                                ));
                            }
                        }
                        _ => out.push(Violation::new(
                            "provenance-required",
                            format!(
                                "{}: a sub-entity carries derived provenance (from, reasoning)",
                                id
                            ),
                        )),
                    }
                    if entity.definition.as_deref().unwrap_or("").trim().is_empty() {
                        out.push(Violation::new(
                            "definition-required",
                            format!(
                                "{}: the definition is the sentence the documents should gain",
                                id
                            ),
                        ));
                    }
                    let moved = staged.iter().any(|o| match o {
                        Op::UpdateRequirement {
                            entities: Some(e), ..
                        } => e.iter().any(|x| x == id),
                        Op::CreateRequirement { requirement, .. } => {
                            requirement.entities.iter().any(|x| x == id)
                        }
                        Op::UpdateEntity {
                            parent: Some(p), ..
                        } => p == id,
                        _ => false,
                    });
                    if !moved {
                        out.push(Violation::new(
                            "nothing-moved",
                            format!(
                                "{}: no requirement re-pointed and no child re-parented under it",
                                id
                            ),
                        ));
                    }
                    if let Some(p) = &entity.parent {
                        let scope_of =
                            |x: &str| store.graph.entities.get(x).map(|e| e.scope.clone());
                        if let Some(ps) = scope_of(p) {
                            if ps != entity.scope {
                                out.push(Violation::new(
                                    "scope-mismatch",
                                    format!(
                                        "{}: a split never crosses a scope ({} is in {})",
                                        id, p, ps
                                    ),
                                ));
                            }
                        }
                    }
                }
                Op::DeleteEntity { id, .. } | Op::MergeEntities { absorb: id, .. }
                    if !staged_parents.contains(id) =>
                {
                    out.push(Violation::new(
                        "target-removed",
                        format!(
                            "{}: an abstraction adds structure; it never deletes or merges",
                            id
                        ),
                    ));
                }
                _ => {}
            }
        }
        out
    }
    // The fan-out variant's own level faces its count even when the changeset left it
    // alone, and the caps variant's target faces its requirement count against the
    // soft value. Mirrors docs/compiler/goals/abstract-entity.md#the-fan-out-gate and #gate.
    fn goal_gate(&self, cx: &GateContext) -> Option<Violation> {
        fan_out_goal_gate(cx.store, cx.staged, &cx.goal.target)
            .or_else(|| caps_goal_gate(cx.store, cx.staged, &cx.goal.target))
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../../docs/compiler/goals/prompts/abstract-entity.md")
    }
}
