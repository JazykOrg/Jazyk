use super::scene::{flow_note, scene, title_line, Scene};
use super::text::{
    alias_of, document, empty_diagram, identifier, link_suffix, oneline, quoted, state_token,
};
use crate::derive::normalize_state;
use crate::model::*;
use crate::store::Store;
use std::collections::{BTreeMap, BTreeSet};

// ---- flow emitters ----

struct Message {
    from: String,
    to: String,
    rid: String,
}

// An interface resolves to its provider: the entity realizing it (the first by id
// when several do). Anything else is its own receiver.
fn provider(store: &Store, id: &str) -> String {
    let is_interface = store
        .graph
        .entities
        .get(id)
        .and_then(|e| e.stereotype.as_deref())
        .is_some_and(|s| s.eq_ignore_ascii_case("interface"));
    if !is_interface {
        return id.to_string();
    }
    let mut realizers: Vec<&str> = store
        .graph
        .relationships
        .values()
        .flat_map(|r| r.contributions.iter())
        .filter(|c| c.r#type == "realization" && c.b == id)
        .map(|c| c.a.as_str())
        .collect();
    realizers.sort();
    realizers.dedup();
    realizers
        .first()
        .map(|s| s.to_string())
        .unwrap_or_else(|| id.to_string())
}

// A member's message: its first dependency edge (an untyped edge counts), else its
// first edge, else a self-message on its first entity.
// A requirement's edges in message order: the edges whose `a` is the statement's first
// listed entity (its subject) first, then dependencies, then the rest, each group in
// listed order. Mirrors docs/compiler/model/view.md#fields.
fn ordered_edges<'a>(store: &Store, r: &'a Requirement) -> Vec<&'a ReqEdge> {
    let subject = r.entities.first().map(|e| store.resolve_id(e).to_string());
    let rank = |e: &ReqEdge| -> u8 {
        if subject.as_deref() == Some(store.resolve_id(&e.a)) {
            0
        } else if e.rel_type.as_deref().is_none_or(|t| t == "dependency") {
            1
        } else {
            2
        }
    };
    let mut edges: Vec<&ReqEdge> = r.edges.iter().collect();
    edges.sort_by_key(|e| rank(e));
    edges
}

fn message_of(store: &Store, rid: &str) -> Option<Message> {
    let r = store.graph.requirements.get(rid)?;
    let live = |id: &str| -> Option<String> {
        let id = store.resolve_id(id).to_string();
        store.graph.entities.contains_key(&id).then_some(id)
    };
    let edge = ordered_edges(store, r).into_iter().next();
    if let Some(e) = edge {
        if let (Some(a), Some(b)) = (live(&e.a), live(&e.b)) {
            return Some(Message {
                from: a,
                to: provider(store, &b),
                rid: rid.to_string(),
            });
        }
    }
    let first = r.entities.iter().find_map(|e| live(e))?;
    Some(Message {
        from: first.clone(),
        to: first,
        rid: rid.to_string(),
    })
}

// The messages of a flow view, each endpoint lifted to its nearest ancestor among
// the view's level members when the view has a level; an endpoint with none draws as
// itself. Mirrors docs/compiler/diagrams.md#level-views.
fn messages(scene: &Scene) -> Vec<Message> {
    let Some(members) = crate::derive::flow_view_level(scene.store, scene.id)
        .map(|t| crate::derive::level_view_members(scene.store, &t))
    else {
        return raw_messages(scene);
    };
    let store = scene.store;
    let lift = |id: &str| -> String {
        crate::derive::lift_into(store, &members, id).unwrap_or_else(|| id.to_string())
    };
    scene
        .requirements
        .iter()
        .filter_map(|rid| {
            let r = store.graph.requirements.get(rid)?;
            let live = |id: &str| -> Option<String> {
                let id = store.resolve_id(id).to_string();
                store.graph.entities.contains_key(&id).then_some(id)
            };
            // The candidate messages in the terms' order: the subject's edges first,
            // then dependencies, then the other edges, then the self-message on the
            // first entity. The first whose lifted ends differ is the level's message.
            let mut candidates: Vec<(String, String)> = Vec::new();
            for e in ordered_edges(store, r) {
                if let (Some(a), Some(b)) = (live(&e.a), live(&e.b)) {
                    candidates.push((lift(&a), lift(&provider(store, &b))));
                }
            }
            if candidates.is_empty() {
                let first = r.entities.iter().find_map(|e| live(e))?;
                let f = lift(&first);
                candidates.push((f.clone(), f));
            }
            let (from, to) = match candidates.iter().find(|(a, b)| a != b).cloned() {
                Some(pair) => pair,
                None => {
                    // Every edge collapsed into one member: an actor among the
                    // statement's entities that lifts elsewhere initiates the message
                    // toward it. Mirrors docs/compiler/model/view.md#fields.
                    let target = candidates[0].1.clone();
                    let actor = r
                        .entities
                        .iter()
                        .filter_map(|e| live(e))
                        .filter(|e| {
                            store.graph.entities[e]
                                .stereotype
                                .as_deref()
                                .is_some_and(|s| s.eq_ignore_ascii_case("actor"))
                        })
                        .map(|e| lift(&e))
                        .find(|l| *l != target);
                    match actor {
                        Some(a) => (a, target),
                        None => candidates[0].clone(),
                    }
                }
            };
            Some(Message {
                from,
                to,
                rid: rid.to_string(),
            })
        })
        .collect()
}

// The messages of a flow view as the emitter draws them: requirement id, initiator,
// receiver, lifted to the view's level. The card model's steps.
pub fn view_messages(store: &Store, view_id: &str, view: &View) -> Vec<(String, String, String)> {
    let sc = scene(store, view_id, view);
    messages(&sc)
        .into_iter()
        .map(|m| (m.rid, m.from, m.to))
        .collect()
}

fn raw_messages(scene: &Scene) -> Vec<Message> {
    scene
        .requirements
        .iter()
        .filter_map(|r| message_of(scene.store, r))
        .collect()
}

// Participants in order of first appearance.
fn participants(messages: &[Message]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in messages {
        for p in [&m.from, &m.to] {
            if !out.contains(p) {
                out.push(p.clone());
            }
        }
    }
    out
}

// The actors of a flow: the members' entities labeled actor, else the initiators that
// never receive.
fn flow_actors(scene: &Scene, messages: &[Message]) -> Vec<String> {
    let store = scene.store;
    let mut actors: Vec<String> = Vec::new();
    for rid in &scene.requirements {
        for e in &scene.requirement(rid).entities {
            let id = store.resolve_id(e).to_string();
            if store.graph.entities.contains_key(&id)
                && scene.labeled(&id, "actor")
                && !actors.contains(&id)
            {
                actors.push(id);
            }
        }
    }
    // No labeled actor: the use case draws none. A flow among the subject's own parts
    // has no outside initiator. Mirrors docs/compiler/model/view.md#fields.
    let _ = messages;
    actors
}

fn use_case_alias(scene: &Scene) -> String {
    let slug = scene.id.rsplit('/').next().unwrap_or(scene.id);
    identifier(slug, "uc_")
}

pub(super) fn emit_use_case(scene: &Scene) -> String {
    let msgs = messages(scene);
    let note = flow_note(scene, None);
    let uc = use_case_alias(scene);
    let mut out = Vec::new();
    let actors = flow_actors(scene, &msgs);
    // An actor links to its card like any rendered entity.
    for a in &actors {
        out.push(format!(
            "actor {} as {}{}",
            quoted(&scene.name(a)),
            alias_of(a),
            link_suffix(a)
        ));
    }
    out.push(format!("usecase {} as {}", quoted(&scene.view.title), uc));
    for a in &actors {
        out.push(format!("{} -- {}", alias_of(a), uc));
    }
    // A use case nobody initiates says so on the picture instead of drawing one bare
    // bubble: the members name no «actor» (docs/compiler/diagrams.md#the-emitters).
    if actors.is_empty() {
        out.push(format!(
            "note bottom of {} : no actor: none of the {} member statement(s) names an «actor» entity",
            uc,
            scene.requirements.len()
        ));
    }
    document(title_line(scene.view, note.as_deref()), out)
}

fn action_label(scene: &Scene, rid: &str) -> String {
    scene.flow_label(rid).replace(';', ",")
}

fn condition_text(s: &str) -> String {
    oneline(s).replace(['(', ')'], "")
}

// Consecutive members whose transitions share subject and from render as one decision.
fn activity_steps(scene: &Scene, members: &[String]) -> Vec<String> {
    let store = scene.store;
    let key = |rid: &str| -> Option<(String, String)> {
        scene.requirement(rid).transition.as_ref().map(|t| {
            (
                store.resolve_id(&t.subject).to_string(),
                normalize_state(&t.from),
            )
        })
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < members.len() {
        let run_end = match key(&members[i]) {
            Some(k) => {
                let mut j = i + 1;
                while j < members.len() && key(&members[j]).as_ref() == Some(&k) {
                    j += 1;
                }
                j
            }
            None => i + 1,
        };
        if run_end - i >= 2 {
            let t = scene.requirement(&members[i]).transition.as_ref().unwrap();
            let cond = t.trigger.clone().unwrap_or_else(|| t.to.clone());
            out.push(format!("if ({}?) then (yes)", condition_text(&cond)));
            out.push(format!("  :{};", action_label(scene, &members[i])));
            out.push("else (no)".to_string());
            for m in &members[i + 1..run_end] {
                out.push(format!("  :{};", action_label(scene, m)));
            }
            out.push("endif".to_string());
        } else {
            out.push(format!(":{};", action_label(scene, &members[i])));
        }
        i = run_end;
    }
    out
}

pub(super) fn emit_activity(scene: &Scene) -> String {
    let note = flow_note(scene, None);
    let mut out = vec!["start".to_string()];
    out.extend(activity_steps(scene, &scene.requirements));
    out.push("stop".to_string());
    document(title_line(scene.view, note.as_deref()), out)
}

pub(super) fn emit_state(scene: &Scene) -> String {
    let Some(subject) = scene.entities.first() else {
        return empty_diagram(scene.view, "no subject entity");
    };
    let mut out = Vec::new();
    let Some(m) = scene.machine(subject) else {
        out.push(format!(
            "state {} as {}",
            quoted(&scene.name(subject)),
            alias_of(subject)
        ));
        return document(None, out);
    };
    let mut tokens: BTreeMap<String, String> = BTreeMap::new();
    for s in &m.states {
        let (decl, token) = state_token(s);
        if let Some(d) = decl {
            out.push(d);
        }
        tokens.insert(normalize_state(s), token);
    }
    let token = |s: &str| {
        tokens
            .get(&normalize_state(s))
            .cloned()
            .unwrap_or_else(|| state_token(s).1)
    };
    if let Some(initial) = m.initial.as_deref() {
        out.push(format!("[*] --> {}", token(initial)));
    }
    for t in &m.transitions {
        let mut line = format!("{} --> {}", token(&t.from), token(&t.to));
        let label = match (t.trigger.as_deref(), t.guard.as_deref()) {
            (Some(tr), Some(g)) => Some(format!("{} [{}]", oneline(tr), oneline(g))),
            (Some(tr), None) => Some(oneline(tr)),
            (None, Some(g)) => Some(format!("[{}]", oneline(g))),
            (None, None) => None,
        };
        if let Some(l) = label {
            line.push_str(&format!(" : {}", l));
        }
        out.push(line);
    }
    document(None, out)
}

pub(super) fn emit_sequence(scene: &Scene) -> String {
    let msgs = messages(scene);
    let people = participants(&msgs);
    let note = flow_note(scene, Some(people.len()));
    let mut out = Vec::new();
    for p in &people {
        let keyword = if scene.labeled(p, "actor") {
            "actor"
        } else {
            "participant"
        };
        // A participant links to its card; a message carries no link.
        out.push(format!(
            "{} {} as {}{}",
            keyword,
            quoted(&scene.name(p)),
            alias_of(p),
            link_suffix(p)
        ));
    }
    for m in &msgs {
        // A failure-mode step is a branch: its message sits in an `alt` fragment
        // labeled by its condition (the transition's trigger and guard when it has
        // one). Mirrors docs/compiler/diagrams.md#the-emitters.
        let r = scene.requirement(&m.rid);
        let failure = r.facets.iter().any(|f| f.facet == "failure-mode");
        if failure {
            let cond = r
                .transition
                .as_ref()
                .and_then(|t| {
                    let mut c = t.trigger.clone().unwrap_or_default();
                    if let Some(g) = &t.guard {
                        c = if c.is_empty() {
                            format!("[{}]", g)
                        } else {
                            format!("{} [{}]", c, g)
                        };
                    }
                    (!c.is_empty()).then_some(c)
                })
                .unwrap_or_else(|| "failure".to_string());
            out.push(format!("alt {}", condition_text(&cond)));
        }
        out.push(format!(
            "{}{} -> {} : {}",
            if failure { "  " } else { "" },
            alias_of(&m.from),
            alias_of(&m.to),
            scene.flow_label(&m.rid)
        ));
        if failure {
            out.push("end".to_string());
        }
    }
    document(title_line(scene.view, note.as_deref()), out)
}

pub(super) fn emit_communication(scene: &Scene) -> String {
    let msgs = messages(scene);
    let people = participants(&msgs);
    let note = flow_note(scene, Some(people.len()));
    let mut out = Vec::new();
    for p in &people {
        let keyword = if scene.labeled(p, "actor") {
            "actor"
        } else {
            "rectangle"
        };
        out.push(format!(
            "{} {} as {}{}",
            keyword,
            quoted(&scene.name(p)),
            alias_of(p),
            link_suffix(p)
        ));
    }
    for (i, m) in msgs.iter().enumerate() {
        out.push(format!(
            "{} -> {} : {}. {}",
            alias_of(&m.from),
            alias_of(&m.to),
            i + 1,
            scene.flow_label(&m.rid)
        ));
    }
    document(title_line(scene.view, note.as_deref()), out)
}

// The leading number of a measure: "2 seconds" → "2".
fn leading_number(measure: &str) -> Option<String> {
    let n: String = measure
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    (!n.is_empty() && n.chars().any(|c| c.is_ascii_digit())).then_some(n)
}

pub(super) fn emit_timing(scene: &Scene) -> String {
    let note = flow_note(scene, None);
    let governing = scene.requirements.iter().find_map(|rid| {
        scene
            .requirement(rid)
            .facets
            .iter()
            .find(|f| f.facet == "quality" && f.measure.is_some())
            .map(|f| (rid.clone(), f.measure.clone().unwrap_or_default()))
    });
    let mut lanes: Vec<&String> = scene
        .entities
        .iter()
        .filter(|e| scene.machine(e).is_some())
        .collect();
    if lanes.is_empty() {
        lanes.extend(scene.entities.iter().take(1));
    }
    if lanes.is_empty() {
        return empty_diagram(scene.view, "no subject entity");
    }
    let title = match (&governing, &note) {
        (Some((rid, _)), Some(n)) => Some(format!("{} {}", scene.flow_label(rid), n)),
        (Some((rid, _)), None) => Some(scene.flow_label(rid)),
        (None, n) => title_line(scene.view, n.as_deref()),
    };
    let mut out = Vec::new();
    for lane in &lanes {
        out.push(format!(
            "robust {} as {}",
            quoted(&scene.name(lane)),
            alias_of(lane)
        ));
    }
    let state_text = |s: &str| {
        let (decl, token) = state_token(s);
        if decl.is_some() {
            quoted(s)
        } else {
            token
        }
    };
    if let Some((_, measure)) = &governing {
        out.push("@0".to_string());
        for lane in &lanes {
            if let Some(initial) = scene.machine(lane).and_then(|m| m.initial.as_deref()) {
                out.push(format!("{} is {}", alias_of(lane), state_text(initial)));
            }
        }
        if let Some(n) = leading_number(measure) {
            out.push(format!("@{}", n));
            for lane in &lanes {
                let Some(m) = scene.machine(lane) else {
                    continue;
                };
                let Some(initial) = m.initial.as_deref() else {
                    continue;
                };
                if let Some(t) = m
                    .transitions
                    .iter()
                    .find(|t| normalize_state(&t.from) == normalize_state(initial))
                {
                    out.push(format!("{} is {}", alias_of(lane), state_text(&t.to)));
                }
            }
        }
    }
    document(title, out)
}

pub(super) fn emit_overview(scene: &Scene) -> String {
    let store = scene.store;
    let note = flow_note(scene, None);
    let member_set: BTreeSet<&str> = scene.requirements.iter().map(String::as_str).collect();
    // (first member index, view id, covered members) per sequence view held by this view.
    let mut refs: Vec<(usize, String, Vec<String>)> = Vec::new();
    for (id, v) in &store.graph.views {
        if v.kind != "sequence" {
            continue;
        }
        let ms: Vec<String> = v
            .members
            .iter()
            .map(|m| store.resolve_id(m).to_string())
            .filter(|m| store.graph.requirements.contains_key(m))
            .collect();
        if ms.is_empty() || !ms.iter().all(|m| member_set.contains(m.as_str())) {
            continue;
        }
        let first = ms
            .iter()
            .filter_map(|m| scene.requirements.iter().position(|r| r == m))
            .min()
            .unwrap_or(0);
        refs.push((first, id.clone(), ms));
    }
    refs.sort();
    let mut covered: BTreeSet<String> = BTreeSet::new();
    let mut out = vec!["start".to_string()];
    for (i, rid) in scene.requirements.iter().enumerate() {
        let mut referenced = false;
        for (_, id, ms) in refs.iter().filter(|(f, _, _)| *f == i) {
            if ms.iter().any(|m| covered.contains(m)) {
                continue;
            }
            out.push(format!(":ref: {};", id));
            covered.extend(ms.iter().cloned());
            referenced = true;
        }
        if referenced || covered.contains(rid) {
            continue;
        }
        out.push(format!(":{};", action_label(scene, rid)));
    }
    out.push("stop".to_string());
    document(title_line(scene.view, note.as_deref()), out)
}
