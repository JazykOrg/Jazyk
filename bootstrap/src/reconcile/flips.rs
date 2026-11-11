use super::*;

// ---- flip detection ----

// One journaled mutation as (op name in snake case, body). The journal writes ops
// tagged (`{op: update_entity, ...}`); an externally tagged form (`{UpdateEntity: {...}}`)
// reads the same, so a replay never depends on which shape an entry took.
fn journal_op(m: &serde_json::Value) -> Option<(String, &serde_json::Value)> {
    let o = m.as_object()?;
    if let Some(op) = o.get("op").and_then(|v| v.as_str()) {
        return Some((op.to_string(), m));
    }
    let (op, body) = o.iter().next()?;
    let mut snake = String::new();
    for (i, c) in op.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                snake.push('_');
            }
            snake.push(c.to_ascii_lowercase());
        } else {
            snake.push(c);
        }
    }
    Some((snake, body))
}

// Cross-class flip detection over the journal: a natural key a GC commit and a compile
// commit hand back and forth. Two flips park the pair and file one
// `unstable-derivation` diagnostic carrying both justifications.
// Mirrors docs/compiler/reconciler.md#flip-detection.
pub fn flip_detection(store: &Store) -> (Vec<Finding>, Vec<ParkedGoal>) {
    flip_detection_over(store, &goals::read_journal(&store.out))
}

// Flip detection over journal entries read once: the cross-class replay and the
// reparent replay share them (docs/compiler/graph.md#journal).
fn flip_detection_over(store: &Store, entries: &[JournalEntry]) -> (Vec<Finding>, Vec<ParkedGoal>) {
    struct Event {
        generation: u64,
        class: goals::Class,
        goal: String,
        justification: String,
    }
    let key_of_entity =
        |name: &str, scope: &str| format!("{}|{}", crate::store::normalize_statement(name), scope);
    let mut id_key: BTreeMap<String, String> = store
        .graph
        .entities
        .iter()
        .map(|(id, e)| (id.clone(), key_of_entity(&e.name, &e.scope)))
        .collect();
    let mut history: BTreeMap<String, Vec<Event>> = BTreeMap::new();
    for entry in entries {
        if entry.kind != "session" {
            continue;
        }
        let goal_of_class = |class: goals::Class| -> Option<String> {
            entry
                .batch
                .iter()
                .find(|g| {
                    goals::parse_goal_id(g)
                        .and_then(|(k, _)| goals::kind(k))
                        .is_some_and(|k| k.class() == class)
                })
                .cloned()
        };
        let class = if goal_of_class(goals::Class::Gc).is_some() {
            goals::Class::Gc
        } else {
            goals::Class::Compile
        };
        let Some(goal) = goal_of_class(class) else {
            continue;
        };
        let justification = entry
            .resolved_goals
            .iter()
            .find(|r| r.goal == goal)
            .map(|r| r.justification.clone())
            .unwrap_or_default();
        for m in &entry.mutations {
            let Some((op, body)) = journal_op(m) else {
                continue;
            };
            let key = match op.as_str() {
                "create_entity" => {
                    let id = body["id"].as_str().unwrap_or("").to_string();
                    let name = body["entity"]["name"].as_str().unwrap_or("");
                    let scope = body["entity"]["scope"].as_str().unwrap_or("public");
                    let key = key_of_entity(name, scope);
                    id_key.insert(id, key.clone());
                    Some(key)
                }
                "delete_entity" | "retract_decree" => {
                    id_key.get(body["id"].as_str().unwrap_or("")).cloned()
                }
                "merge_entities" => id_key.get(body["absorb"].as_str().unwrap_or("")).cloned(),
                _ => None,
            };
            if let Some(key) = key {
                history.entry(key).or_default().push(Event {
                    generation: entry.generation,
                    class,
                    goal: goal.clone(),
                    justification: justification.clone(),
                });
            }
        }
    }
    let mut findings = Vec::new();
    let mut parked = Vec::new();
    for (key, events) in history {
        let flips = events
            .windows(2)
            .filter(|w| w[0].class != w[1].class)
            .count();
        if flips < 2 {
            continue;
        }
        let last = events.last().unwrap();
        let gc = events
            .iter()
            .rev()
            .find(|e| e.class == goals::Class::Gc)
            .unwrap();
        let compile = events
            .iter()
            .rev()
            .find(|e| e.class == goals::Class::Compile)
            .unwrap();
        let subject = id_key
            .iter()
            .filter(|(id, k)| **k == key && store.graph.entities.contains_key(*id))
            .map(|(id, _)| id.clone())
            .next()
            .or_else(|| {
                goals::parse_goal_id(&compile.goal)
                    .map(|(_, t)| t.to_string())
                    .filter(|t| store.graph.entities.contains_key(t))
            });
        let Some(subject) = subject else { continue };
        // A ruling already given, or a filing newer than the last flip, ends it.
        let settled = store.graph.diagnostics.values().any(|d| {
            d.rule == "unstable-derivation"
                && d.subjects.contains(&subject)
                && (d.answer.is_some()
                    || d.updated
                        .as_deref()
                        .and_then(|b| b.strip_prefix('g'))
                        .and_then(|n| n.parse::<u64>().ok())
                        .is_some_and(|n| n >= last.generation && d.lifecycle != "open"))
        });
        if settled {
            continue;
        }
        let name = key.split('|').next().unwrap_or(&key).to_string();
        let prompt = DiagnosticPrompt {
            question: format!(
                "`{}` flips between the classes: {} ({}) and {} ({}). Which direction holds?",
                name,
                gc.goal,
                gc.class.name(),
                compile.goal,
                compile.class.name()
            ),
            options: vec![
                PromptOption {
                    label: format!("Keep the split ({})", gc.goal),
                    edit: None,
                    answer: Some(format!(
                        "Keep the structure {} derived; the merge is wrong.",
                        gc.goal
                    )),
                },
                PromptOption {
                    label: format!("Keep the merge ({})", compile.goal),
                    edit: None,
                    answer: Some(format!(
                        "Keep the merge {} made; the split is wrong.",
                        compile.goal
                    )),
                },
            ],
            freeform: true,
        };
        findings.push((
            "unstable-derivation".to_string(),
            vec![subject.clone()],
            "warning".to_string(),
            format!(
                "`{}` was handed back and forth between {} (g{}: {}) and {} (g{}: {}); both goals are parked until a ruling",
                name,
                gc.goal,
                gc.generation,
                if gc.justification.is_empty() { "no justification recorded" } else { &gc.justification },
                compile.goal,
                compile.generation,
                if compile.justification.is_empty() { "no justification recorded" } else { &compile.justification }
            ),
            Some(prompt),
        ));
        for (goal, gen) in [
            (&gc.goal, gc.generation),
            (&compile.goal, compile.generation),
        ] {
            let Some((kind, target)) = goals::parse_goal_id(goal) else {
                continue;
            };
            let class = goals::kind(kind)
                .map(|k| k.class())
                .unwrap_or(goals::Class::Compile);
            let parked_goal = Goal {
                id: goal.clone(),
                kind: kind.to_string(),
                class: class.name().to_string(),
                mandatory: class == goals::Class::Compile,
                target: target.to_string(),
                unit: goals::kind(kind)
                    .map(|k| k.unit())
                    .unwrap_or("node")
                    .to_string(),
                change: json!({"unstable": name, "subject": subject}),
                cause: Some(Cause {
                    generation: gen,
                    mutation: 0,
                    via: "flip-detection".into(),
                }),
                // Blocked on the ruling, never batched: a parked goal resumes first,
                // and a session cannot resolve one whose cause is the ruling owed
                // (docs/compiler/reconciler.md#flip-detection).
                state: GoalState::Blocked {
                    on: format!("human: unstable-derivation ruling on {}", subject),
                },
                hints: vec![format!(
                    "parked by flip detection; answer the unstable-derivation prompt on {}",
                    subject
                )],
            };
            parked.push(ParkedGoal::new(
                parked_goal,
                format!(
                    "cross-class flip: `{}` was handed back and forth between {} and {}; waits for the unstable-derivation ruling on {}",
                    name, gc.goal, compile.goal, subject
                ),
            ));
        }
    }
    let (reparent_findings, reparent_parked) = reparent_flips(store, entries);
    findings.extend(reparent_findings);
    parked.extend(reparent_parked);
    (findings, parked)
}

// The reparent flip: a child that moves between the same two parents across
// generations. The store's replay of the journal (`journaled_parent_moves`: an
// `update_entity` or `retract_decree` carrying `parent` and the prior parent, each
// child of a `dissolve_entity`, a tool's or the sweep's) becomes a per-child list of
// moves keyed on natural keys, so a grouping dissolved and re-minted under a new id
// counts as the same parent and a parentless side is the scope root. A move back over
// an earlier move parks the goal behind the second move and files one
// unstable-derivation diagnostic on the child with both justifications and a prompt:
// the first parent, the second, or a freeform ruling. A `reparent-flip` change record
// the store wrote on a child names the pair when the replay alone cannot. Mirrors
// docs/compiler/reconciler.md#flip-detection.
fn reparent_flips(store: &Store, entries: &[JournalEntry]) -> (Vec<Finding>, Vec<ParkedGoal>) {
    struct Move {
        generation: u64,
        // The parent the move landed on; the prior parent rides in `from_key` only.
        to: Option<String>,
        from_key: String,
        to_key: String,
        goal: Option<String>,
        justification: String,
    }
    let key_of_entity =
        |name: &str, scope: &str| format!("{}|{}", crate::store::normalize_statement(name), scope);
    // Natural keys: the live entities, then the journal's creates for the dead ones.
    let mut id_key: BTreeMap<String, String> = store
        .graph
        .entities
        .iter()
        .map(|(id, e)| (id.clone(), key_of_entity(&e.name, &e.scope)))
        .collect();
    for entry in entries {
        for m in &entry.mutations {
            let Some((op, body)) = journal_op(m) else {
                continue;
            };
            if op != "create_entity" {
                continue;
            }
            let Some(id) = body["id"].as_str() else {
                continue;
            };
            let name = body["entity"]["name"].as_str().unwrap_or("");
            let scope = body["entity"]["scope"].as_str().unwrap_or("public");
            id_key
                .entry(id.to_string())
                .or_insert_with(|| key_of_entity(name, scope));
        }
    }
    let scope_of = |child: &str| -> String {
        store
            .graph
            .entities
            .get(child)
            .map(|e| e.scope.clone())
            .or_else(|| {
                id_key
                    .get(child)
                    .and_then(|k| k.split('|').nth(1))
                    .map(String::from)
            })
            .unwrap_or_else(|| Entity::default().scope)
    };
    let mut history: BTreeMap<String, Vec<Move>> = BTreeMap::new();
    for mv in crate::store::parent_moves_in(entries) {
        let scope = scope_of(&mv.child);
        let parent_key = |p: Option<&str>| -> String {
            match p {
                None => crate::store::scope_root_target(&scope),
                Some(p) => id_key.get(p).cloned().unwrap_or_else(|| p.to_string()),
            }
        };
        let from_key = parent_key(mv.from.as_deref());
        let to_key = parent_key(mv.to.as_deref());
        if from_key == to_key {
            continue;
        }
        // The goal behind the move: the session's batch goal on the child or one of
        // its parents, else the batch's first; a store-level entry (the sweep, a
        // decree) has none, and its note stands in for a justification.
        let entry = entries.iter().find(|e| e.generation == mv.generation);
        let goal = entry.filter(|e| e.kind == "session").and_then(|e| {
            let near = |g: &&String| {
                goals::parse_goal_id(g).is_some_and(|(_, t)| {
                    t == mv.child
                        || mv.from.as_deref() == Some(t)
                        || mv.to.as_deref() == Some(t)
                        || t == from_key
                        || t == to_key
                })
            };
            e.batch
                .iter()
                .find(near)
                .or_else(|| e.batch.first())
                .cloned()
        });
        let justification = entry
            .and_then(|e| {
                goal.as_deref()
                    .and_then(|g| e.resolved_goals.iter().find(|r| r.goal == g))
                    .map(|r| r.justification.clone())
                    .filter(|j| !j.trim().is_empty())
                    .or_else(|| e.note.clone())
                    .or_else(|| Some(format!("{} entry, no justification recorded", e.kind)))
            })
            .unwrap_or_else(|| "no journal entry for the move".to_string());
        let child_key = id_key
            .get(&mv.child)
            .cloned()
            .unwrap_or_else(|| mv.child.clone());
        history.entry(child_key).or_default().push(Move {
            generation: mv.generation,
            from_key,
            to_key,
            to: mv.to.clone(),
            goal,
            justification,
        });
    }
    // The store's own record names a child whose journaled moves the replay may not
    // see whole; it adds the child to the candidates with its pair of parents.
    let recorded: BTreeMap<String, (String, String)> = store
        .status
        .changes
        .iter()
        .filter(|c| c.kind == crate::store::CHANGE_REPARENT_FLIP)
        .filter_map(|c| {
            let between = c.detail["between"].as_array()?;
            let a = between
                .first()
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let b = between
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some((store.resolve_id(&c.subject).to_string(), (a, b)))
        })
        .collect();
    let mut findings = Vec::new();
    let mut parked = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let current_id = |key: &str| -> Option<String> {
        id_key
            .iter()
            .filter(|(id, k)| k.as_str() == key && store.graph.entities.contains_key(*id))
            .map(|(id, _)| id.clone())
            .next()
    };
    let label = |id: Option<&str>| -> String {
        match id {
            None => "the scope root".to_string(),
            Some(p) => store
                .graph
                .entities
                .get(store.resolve_id(p))
                .map(|e| format!("`{}`", e.name))
                .unwrap_or_else(|| format!("`{}`", p)),
        }
    };
    let mut candidates: Vec<(String, Option<(usize, usize)>)> = Vec::new();
    for (key, moves) in &history {
        let Some(subject) = current_id(key) else {
            continue;
        };
        let mut pair = None;
        for j in (1..moves.len()).rev() {
            if let Some(i) = (0..j).rev().find(|&i| {
                moves[i].from_key == moves[j].to_key && moves[i].to_key == moves[j].from_key
            }) {
                pair = Some((i, j));
                break;
            }
        }
        if pair.is_some() || recorded.contains_key(&subject) {
            candidates.push((subject, pair));
        }
    }
    for (subject, pair) in candidates {
        if !seen.insert(subject.clone()) {
            continue;
        }
        let key = id_key.get(&subject).cloned().unwrap_or_default();
        let moves = history.get(&key).map(|m| m.as_slice()).unwrap_or(&[]);
        let (first, second) = match pair {
            Some((i, j)) => (Some(&moves[i]), Some(&moves[j])),
            None => (None, moves.last()),
        };
        let last_generation = second.map(|m| m.generation).unwrap_or(0);
        // A ruling already given, or a filing newer than the last move, ends it.
        let settled = store.graph.diagnostics.values().any(|d| {
            d.rule == "unstable-derivation"
                && d.subjects.contains(&subject)
                && (d.answer.is_some()
                    || d.updated
                        .as_deref()
                        .and_then(|b| b.strip_prefix('g'))
                        .and_then(|n| n.parse::<u64>().ok())
                        .is_some_and(|n| n >= last_generation && d.lifecycle != "open"))
        });
        if settled {
            continue;
        }
        // The two parents: where the first move put the child, and where the second
        // moved it back; the record's pair when the replay has no matched moves.
        let (parent_a, parent_b): (Option<String>, Option<String>) = match (first, second) {
            (Some(a), Some(b)) => (a.to.clone(), b.to.clone()),
            _ => {
                let (a, b) = recorded.get(&subject).cloned().unwrap_or_default();
                let opt = |s: String| (!s.is_empty()).then_some(s);
                (opt(a), opt(b))
            }
        };
        let name = store
            .graph
            .entities
            .get(&subject)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| subject.clone());
        let describe = |m: Option<&Move>, parent: Option<&str>| -> String {
            match m {
                Some(m) => format!(
                    "{} (g{}: {}) put it under {}",
                    m.goal.as_deref().unwrap_or("the store"),
                    m.generation,
                    m.justification,
                    label(parent)
                ),
                None => format!("an earlier generation put it under {}", label(parent)),
            }
        };
        let prompt = DiagnosticPrompt {
            question: format!(
                "`{}` moves back and forth between {} and {}. Which parent holds?",
                name,
                label(parent_a.as_deref()),
                label(parent_b.as_deref())
            ),
            options: vec![
                PromptOption {
                    label: format!("Keep it under {}", label(parent_a.as_deref())),
                    edit: None,
                    answer: Some(format!(
                        "Keep {} under {}; the move back is wrong.",
                        subject,
                        parent_a.as_deref().unwrap_or("the scope root")
                    )),
                },
                PromptOption {
                    label: format!("Keep it under {}", label(parent_b.as_deref())),
                    edit: None,
                    answer: Some(format!(
                        "Keep {} under {}; the first move is wrong.",
                        subject,
                        parent_b.as_deref().unwrap_or("the scope root")
                    )),
                },
            ],
            freeform: true,
        };
        findings.push((
            "unstable-derivation".to_string(),
            vec![subject.clone()],
            "warning".to_string(),
            format!(
                "`{}` was reparented back and forth: {}; {}; the second move is parked until a ruling",
                name,
                describe(first, parent_a.as_deref()),
                describe(second, parent_b.as_deref())
            ),
            Some(prompt),
        ));
        let Some(second) = second else { continue };
        let Some(goal) = second.goal.as_deref() else {
            continue;
        };
        let Some((kind, target)) = goals::parse_goal_id(goal) else {
            continue;
        };
        let class = goals::kind(kind)
            .map(|k| k.class())
            .unwrap_or(goals::Class::Compile);
        let reason = format!(
            "reparent flip: `{}` was moved back and forth between {} and {}; waits for the unstable-derivation ruling",
            subject,
            parent_a.as_deref().unwrap_or("the scope root"),
            parent_b.as_deref().unwrap_or("the scope root")
        );
        let parked_goal = Goal {
            id: goal.to_string(),
            kind: kind.to_string(),
            class: class.name().to_string(),
            mandatory: class == goals::Class::Compile,
            target: target.to_string(),
            unit: goals::kind(kind)
                .map(|k| k.unit())
                .unwrap_or("node")
                .to_string(),
            change: json!({
                "reparent": subject,
                "between": [parent_a, parent_b],
            }),
            cause: Some(Cause {
                generation: second.generation,
                mutation: 0,
                via: "flip-detection".into(),
            }),
            // Blocked on the ruling, never batched (the novel's conform-instance goal
            // behind a reparent flip was batched, resolved, and re-derived six times
            // in one build with no mutation between).
            state: GoalState::Blocked {
                on: format!("human: unstable-derivation ruling on {}", subject),
            },
            hints: vec![format!(
                "parked by flip detection; answer the unstable-derivation prompt on {}",
                subject
            )],
        };
        parked.push(ParkedGoal::new(parked_goal, reason));
    }
    (findings, parked)
}

#[cfg(test)]
mod tests;
