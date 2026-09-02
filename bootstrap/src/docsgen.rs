// Deterministic per-entity requirements documents: the reading surface between prose
// and graph. Rendered on every commit, no LLM. Renders the diagrams first so every
// image link resolves, embeds the views relevant to each entity, and cross-links the
// pages through the views they share. Mirrors docs/consumers/docsgen.md.
use crate::derive::{entity_slug, instance_types, view_edge_count};
use crate::gen::GenSettings;
use crate::model::{Diagnostic, Goal, Provenance, StateMachine, View};
use crate::store::Store;
use std::collections::{BTreeMap, BTreeSet};

fn slug(id: &str) -> String {
    id.strip_prefix("ent:").unwrap_or(id).to_string()
}

// "[Name](./slug.md)" for a live entity, the bare id otherwise.
fn page_link(store: &Store, id: &str) -> String {
    let resolved = store.resolve_id(id);
    match store.graph.entities.get(resolved) {
        Some(e) => format!("[{}](./{}.md)", e.name, slug(resolved)),
        None => format!("`{}`", id),
    }
}

// The relative paths an entity page links: `../diagrams/<kind>/<slug>.svg` and `.puml`.
fn diagram_rel(view_id: &str) -> Option<(String, String)> {
    let rel = view_id.strip_prefix("view:")?;
    let (kind, s) = rel.split_once('/')?;
    Some((
        format!("../diagrams/{}/{}.svg", kind, s),
        format!("../diagrams/{}/{}.puml", kind, s),
    ))
}

fn svg_exists(store: &Store, view_id: &str) -> bool {
    let Some(rel) = view_id.strip_prefix("view:") else {
        return false;
    };
    let Some((kind, s)) = rel.split_once('/') else {
        return false;
    };
    store
        .out
        .join("diagrams")
        .join(kind)
        .join(format!("{}.svg", s))
        .exists()
}

const FLOW_KINDS: [&str; 4] = ["use-case", "activity", "sequence", "communication"];

// The arrows a view renders, counted over the members' relationships.
fn edge_count(store: &Store, view: &View) -> u64 {
    let members: Vec<String> = view
        .members
        .iter()
        .map(|m| store.resolve_id(m).to_string())
        .collect();
    view_edge_count(store, &members)
}

// The `split-view` goal a view carries when a count crossed its (possibly bumped)
// soft threshold: the member count for its kind, the edge count on structural and
// object views, the participant count on sequence and communication views. Mirrors
// docs/consumers/docsgen.md#diagrams-on-entity-pages.
fn over_limit_goal(store: &Store, view_id: &str, view: &View) -> Option<String> {
    let over = |name: &str, count: u64| -> bool {
        let bump = view.limits.get(name).map(|b| b.value);
        match crate::limits::threshold(name, bump) {
            Some((soft, _)) => count > soft,
            None => false,
        }
    };
    let members = view.members.len() as u64;
    let crossed = match view.kind.as_str() {
        "object" => {
            over("instances-per-object-view", members)
                || over("edges-per-view", edge_count(store, view))
        }
        k if FLOW_KINDS.contains(&k) => {
            over("members-per-flow-view", members)
                || ((k == "sequence" || k == "communication")
                    && over(
                        "participants-per-sequence-view",
                        participants(store, view).len() as u64,
                    ))
        }
        _ => {
            over("members-per-structural-view", members)
                || over("edges-per-view", edge_count(store, view))
        }
    };
    if crossed {
        return Some(format!("g:split-view:{}", view_id));
    }
    None
}

// The state machine whose subject is the first member of a state view.
fn machine_for<'a>(store: &'a Store, view: &View) -> Option<&'a StateMachine> {
    let subject = view.members.first()?;
    store
        .graph
        .state_machines
        .values()
        .find(|m| &m.subject == subject)
}

// The participant entities of a flow view, in order of first appearance across its
// member requirements.
fn participants(store: &Store, view: &View) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in &view.members {
        let Some(r) = store.graph.requirements.get(store.resolve_id(m)) else {
            continue;
        };
        for e in &r.entities {
            let resolved = store.resolve_id(e).to_string();
            if store.graph.entities.contains_key(&resolved) && !out.contains(&resolved) {
                out.push(resolved);
            }
        }
    }
    out
}

// The caption line under an embedded rendering: the view id, its kind and count, the
// member entities as links to their pages, and the `.puml` source. The links are the
// cross-links between entity pages. Mirrors docs/consumers/docsgen.md#diagrams-on-entity-pages.
fn caption(store: &Store, view_id: &str, view: &View) -> String {
    let (_, puml) = diagram_rel(view_id).unwrap_or_default();
    let mut line = format!("`{}` (", view_id);
    let mut listed: Vec<String> = Vec::new();
    if view.kind == "state" {
        match machine_for(store, view) {
            Some(m) => line.push_str(&format!(
                "state, {} states: {}",
                m.states.len(),
                m.states.join(", ")
            )),
            None => line.push_str(&format!("state, {} members", view.members.len())),
        }
    } else if FLOW_KINDS.contains(&view.kind.as_str()) {
        line.push_str(&format!("{}, {} steps", view.kind, view.members.len()));
        listed = participants(store, view);
    } else {
        line.push_str(&format!("{}, {} members", view.kind, view.members.len()));
        listed = view
            .members
            .iter()
            .map(|m| store.resolve_id(m).to_string())
            .filter(|m| store.graph.entities.contains_key(m))
            .collect();
    }
    line.push(')');
    if !listed.is_empty() {
        let mut links: Vec<String> = listed.iter().take(8).map(|m| page_link(store, m)).collect();
        if listed.len() > 8 {
            links.push("...".to_string());
        }
        line.push_str(&format!(": {}", links.join(", ")));
    }
    line.push_str(&format!(" · [source]({})", puml));
    if let Some(goal) = over_limit_goal(store, view_id, view) {
        line.push_str(&format!(" · goal `{}`", goal));
    }
    line
}

// One embedded rendering: the image when its `.svg` exists, the caption either way.
// A view whose render failed keeps its caption and `.puml` link; nothing is invented.
fn embed(store: &Store, view_id: &str, view: &View) -> String {
    let Some((svg, _)) = diagram_rel(view_id) else {
        return String::new();
    };
    let mut s = String::new();
    if svg_exists(store, view_id) {
        s.push_str(&format!("![{}]({})\n\n", view.title, svg));
    }
    s.push_str(&caption(store, view_id, view));
    s.push_str("\n\n");
    s
}

// The synthesized state view of an entity's machine, when no stored view exists (the
// renderer synthesizes the same one). Mirrors render targets.
fn synthesized_state_view(store: &Store, id: &str) -> Option<View> {
    let m = store
        .graph
        .state_machines
        .values()
        .find(|m| m.subject == id)?;
    let name = store
        .graph
        .entities
        .get(&m.subject)
        .map(|e| e.name.clone())?;
    Some(View {
        kind: "state".to_string(),
        title: name,
        members: vec![m.subject.clone()],
        ..Default::default()
    })
}

// The views an entity page embeds, in the order the docs state: the level neighborhood
// (the parent's level view, or the scope root's, then the entity's own), the entity's
// own state view, every flow view naming it, the object view of its type, then every
// other curated view listing it.
// Mirrors docs/consumers/docsgen.md#diagrams-on-entity-pages.
fn relevant_views(
    store: &Store,
    id: &str,
    instances: &BTreeMap<String, String>,
) -> Vec<(String, View)> {
    let ent = &store.graph.entities[id];
    let mut out: Vec<(String, View)> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut push = |vid: String, view: View, out: &mut Vec<(String, View)>| {
        if seen.insert(vid.clone()) {
            out.push((vid, view));
        }
    };
    let stored = |vid: &str| store.graph.views.get(vid).cloned();

    // The level neighborhood: the parent's level view (the scope root's for a
    // parentless entity), then the entity's own level view when it holds a level.
    // Mirrors docs/consumers/docsgen.md#diagrams-on-entity-pages.
    let above = match ent.parent.as_deref() {
        Some(p) => store.resolve_id(p).to_string(),
        None => crate::store::scope_root_target(&ent.scope),
    };
    for target in [above, id.to_string()] {
        if let Some(vid) = crate::derive::level_view_id(store, &target) {
            if let Some(v) = stored(&vid) {
                push(vid, v, &mut out);
            }
        }
    }
    let state_id = format!("view:state/{}", entity_slug(id));
    match stored(&state_id) {
        Some(v) => push(state_id, v, &mut out),
        None => {
            if let Some(v) = synthesized_state_view(store, id) {
                push(state_id, v, &mut out);
            }
        }
    }
    for (vid, v) in &store.graph.views {
        if !FLOW_KINDS.contains(&v.kind.as_str()) {
            continue;
        }
        let names_it = v.members.iter().any(|m| {
            store
                .graph
                .requirements
                .get(store.resolve_id(m))
                .is_some_and(|r| r.entities.iter().any(|e| store.resolve_id(e) == id))
        });
        if names_it {
            push(vid.clone(), v.clone(), &mut out);
        }
    }
    let ty = instances
        .get(id)
        .cloned()
        .or_else(|| instances.values().any(|t| t == id).then(|| id.to_string()));
    if let Some(t) = ty {
        let ov = format!("view:object/{}", entity_slug(&t));
        if let Some(v) = stored(&ov) {
            push(ov, v, &mut out);
        }
    }
    for (vid, v) in &store.graph.views {
        if v.default {
            continue;
        }
        let listed = v
            .members
            .iter()
            .chain(v.collapse.iter())
            .any(|m| store.resolve_id(m) == id);
        if listed {
            push(vid.clone(), v.clone(), &mut out);
        }
    }
    out
}

fn prov_short(p: &Provenance) -> String {
    match p {
        Provenance::Quote(s) => format!("`{}#{}`", s.doc, s.section),
        Provenance::Derived { from, reasoning } => {
            format!("derived from {} ({})", from.join(", "), reasoning)
        }
        Provenance::Decree { author, at, note } => format!(
            "decreed by {} at {}{}",
            author,
            at,
            note.as_ref()
                .map(|n| format!(" ({})", n))
                .unwrap_or_default()
        ),
    }
}

// Whether an open diagnostic proposes a sentence for this entity, one of its
// attributes, or one of its requirements.
fn proposes_for(store: &Store, d: &Diagnostic, id: &str, rids: &[String]) -> bool {
    if d.lifecycle != "open" {
        return false;
    }
    if d.rule != "ratification-pending" && d.rule != "invented-choice" {
        return false;
    }
    d.subjects.iter().any(|sj| {
        let resolved = store.resolve_id(sj);
        resolved == id || rids.iter().any(|r| r == resolved)
    })
}

// One proposal, rendered as its prompt: the sentence, the target, the upstream nodes
// with their reasoning (or the decree's author and note), and the options.
// Mirrors docs/consumers/docsgen.md#ratification-proposals.
fn render_proposal(did: &str, d: &Diagnostic) -> String {
    let mut s = format!("### `{}`\n\n{}\n\n", did, d.message);
    if let Some(rsn) = &d.reasoning {
        s.push_str(&format!("{}\n\n", rsn));
    }
    let Some(p) = &d.prompt else {
        return s;
    };
    s.push_str(&format!("{}\n\n", p.question));
    for o in &p.options {
        match &o.edit {
            Some(e) => {
                s.push_str(&format!("- Apply: {}\n\n  > {}\n\n", o.label, e.new_text));
                s.push_str(&format!("  Target: `{}#{}`\n", e.doc, e.section));
            }
            None => s.push_str(&format!("- Answer: {}\n", o.label)),
        }
    }
    s.push('\n');
    s
}

// The goals the store still holds (parked and failed persist in status.yaml) whose
// target is the entity, one of its requirements, or a view listing the entity.
fn goal_lines(store: &Store, id: &str, rids: &[String]) -> Vec<String> {
    let matches = |g: &Goal| {
        let t = g.target.as_str();
        t == id
            || rids.iter().any(|r| r == t)
            || store.graph.views.get(t).is_some_and(|v| {
                v.members
                    .iter()
                    .chain(v.collapse.iter())
                    .any(|m| store.resolve_id(m) == id)
            })
    };
    let cause = |g: &Goal| {
        g.cause
            .as_ref()
            .map(|c| format!(" · cause c{}-{} via {}", c.generation, c.mutation, c.via))
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for g in &store.status.parked {
        if matches(g) {
            out.push(format!("- `{}` ({}) parked{}", g.id, g.kind, cause(g)));
        }
    }
    for f in &store.status.failed {
        if matches(&f.goal) {
            out.push(format!(
                "- `{}` ({}) failed: {}{}",
                f.goal.id,
                f.goal.kind,
                f.reason,
                cause(&f.goal)
            ));
        }
    }
    out
}

pub fn write_all(store: &Store, gs: &GenSettings) -> usize {
    // Render the diagrams first, so every image link below resolves even when this
    // run is `jazyk docsgen` alone. Unchanged views are skipped by content.
    crate::render::render_all(store, &store.out);
    let vmap = crate::verify::status_map(store, gs);
    let instances = instance_types(store);
    let dir = store.out.join("docsgen");
    std::fs::create_dir_all(&dir).ok();
    // Stale documents for absent entities are removed, so links never mislead.
    let live: BTreeSet<String> = store.graph.entities.keys().map(|id| slug(id)).collect();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(stem) = name.strip_suffix(".md") {
                if stem != "index" && !live.contains(stem) {
                    std::fs::remove_file(e.path()).ok();
                }
            }
        }
    }
    let mut written = 0;
    for (id, ent) in &store.graph.entities {
        let mut s = String::new();
        s.push_str(&format!("# {}\n\n", ent.name));
        s.push_str(&format!("`{}`", id));
        if let Some(st) = &ent.stereotype {
            s.push_str(&format!(" · «{}»", st));
        }
        if ent.scope != "public" {
            s.push_str(&format!(" · scope `{}`", ent.scope));
        }
        if !ent.aliases.is_empty() {
            s.push_str(&format!(" · also known as: {}", ent.aliases.join(", ")));
        }
        s.push_str("\n\n");
        if let Some(d) = &ent.definition {
            s.push_str(d);
            s.push_str("\n\n");
        }
        let children: Vec<&String> = store
            .graph
            .entities
            .iter()
            .filter(|(_, e)| {
                e.parent
                    .as_deref()
                    .is_some_and(|p| store.resolve_id(p) == id.as_str())
            })
            .map(|(cid, _)| cid)
            .collect();
        if ent.parent.is_some() || !children.is_empty() {
            let mut line = String::new();
            if let Some(p) = &ent.parent {
                line.push_str(&format!("Parent: {}", page_link(store, p)));
            }
            if !children.is_empty() {
                if !line.is_empty() {
                    line.push_str(" · ");
                }
                let links: Vec<String> = children.iter().map(|c| page_link(store, c)).collect();
                line.push_str(&format!("Children: {}", links.join(", ")));
            }
            s.push_str(&line);
            s.push_str("\n\n");
        }
        if !ent.attributes.is_empty() {
            s.push_str("Attributes:\n\n");
            for a in &ent.attributes {
                let mut line = format!("- `{}`", a.name);
                if let Some(t) = &a.r#type {
                    line.push_str(&format!(": {}", t));
                }
                if let Some(v) = &a.value {
                    line.push_str(&format!(" = {}", v));
                }
                line.push_str(&format!(" · {}", prov_short(&a.provenance)));
                s.push_str(&line);
                s.push('\n');
            }
            s.push('\n');
        }
        if let Some(p) = &ent.provenance {
            s.push_str(&format!(
                "This entity is {} ({}); see [Proposals](#proposals).\n\n",
                p.kind(),
                prov_short(p)
            ));
        }

        // Diagrams: the renderings relevant to this entity, embedded as relative
        // links into ../diagrams/, so the out directory serves anywhere as-is.
        let views = relevant_views(store, id, &instances);
        if !views.is_empty() {
            s.push_str("## Diagrams\n\n");
            for (vid, v) in &views {
                s.push_str(&embed(store, vid, v));
            }
        }

        let mut rids = store.requirements_referencing(id);
        rids.sort();
        if !rids.is_empty() {
            s.push_str("## Requirements\n\n");
            for rid in &rids {
                let Some(r) = store.graph.requirements.get(rid) else {
                    continue;
                };
                s.push_str(&format!("### `{}`\n\n{}\n\n", rid, r.statement));
                if !r.facets.is_empty() {
                    let fs: Vec<String> = r
                        .facets
                        .iter()
                        .map(|f| match &f.measure {
                            Some(m) => format!("{} ({})", f.facet, m),
                            None => f.facet.clone(),
                        })
                        .collect();
                    s.push_str(&format!("Facets: {}\n\n", fs.join(", ")));
                }
                if !r.edges.is_empty() {
                    let es: Vec<String> = r
                        .edges
                        .iter()
                        .map(|e| {
                            let t = e
                                .rel_type
                                .as_deref()
                                .unwrap_or(crate::model::DEFAULT_REL_TYPE);
                            match &e.cardinality {
                                Some(c) => format!("`{}` → `{}` ({}, {})", e.a, e.b, t, c),
                                None => format!("`{}` → `{}` ({})", e.a, e.b, t),
                            }
                        })
                        .collect();
                    s.push_str(&format!("Edges: {}\n\n", es.join(", ")));
                }
                if let Some(t) = &r.transition {
                    let mut line = format!("Transition: `{}`: {} → {}", t.subject, t.from, t.to);
                    if let Some(tr) = &t.trigger {
                        line.push_str(&format!(" on {}", tr));
                    }
                    if let Some(gu) = &t.guard {
                        line.push_str(&format!(" if {}", gu));
                    }
                    s.push_str(&line);
                    s.push_str("\n\n");
                }
                match r.source.as_ref() {
                    Some(src) => s.push_str(&format!(
                        "> {}\n\nSource: `{}#{}`",
                        src.quote.split_whitespace().collect::<Vec<_>>().join(" "),
                        src.doc,
                        src.section
                    )),
                    None => s.push_str(&format!(
                        "Provenance: {}",
                        crate::session::provenance_line(r)
                    )),
                }
                let others: Vec<String> = r
                    .entities
                    .iter()
                    .filter(|e| store.resolve_id(e) != id.as_str())
                    .map(|e| page_link(store, e))
                    .collect();
                if !others.is_empty() {
                    s.push_str(&format!(" · ties: {}", others.join(", ")));
                }
                s.push_str("\n\n");
                if let Some(v) = vmap.get(rid.as_str()) {
                    let status = v["status"].as_str().unwrap_or("missing");
                    let mut line = format!("Verification: `{}`", status);
                    if let Some(name) = v["name"].as_str() {
                        line.push_str(&format!(
                            " by `{}` ({})",
                            name,
                            v["kind"].as_str().unwrap_or("?")
                        ));
                    }
                    if let Some(t) = v["lastRun"].as_str() {
                        line.push_str(&format!(", last run {}", t));
                    }
                    if let Some(ev) = v["evidence"].as_str() {
                        line.push_str(&format!(
                            "\n\n> {}",
                            ev.split_whitespace().collect::<Vec<_>>().join(" ")
                        ));
                    }
                    s.push_str(&line);
                    s.push_str("\n\n");
                }
            }
        }

        // Relationships: one line per direction-and-type group, the other member
        // linked to its page. Mirrors docs/consumers/docsgen.md#the-requirements-document.
        let mut rels: Vec<String> = Vec::new();
        for (relid, rel) in &store.graph.relationships {
            if !rel.members.contains(id) {
                continue;
            }
            for c in &rel.contributions {
                let end_txt = |e: &String| {
                    if e == id {
                        ent.name.clone()
                    } else {
                        page_link(store, e)
                    }
                };
                let mut line = format!("- {} → {} ({}", end_txt(&c.a), end_txt(&c.b), c.r#type);
                if let Some(card) = &c.cardinality {
                    line.push_str(&format!(", {}", card));
                }
                line.push_str(&format!(
                    ") · `{}` · from {}",
                    relid,
                    c.requirements.join(", ")
                ));
                rels.push(line);
            }
        }
        if !rels.is_empty() {
            s.push_str("## Relationships\n\n");
            s.push_str(&rels.join("\n"));
            s.push_str("\n\n");
        }

        // Proposals: pending ratifications on the entity, its attributes, and its
        // requirements, rendered as prompts with the suggested edit.
        let proposals: Vec<String> = store
            .graph
            .diagnostics
            .iter()
            .filter(|(_, d)| proposes_for(store, d, id, &rids))
            .map(|(did, d)| render_proposal(did, d))
            .collect();
        if !proposals.is_empty() {
            s.push_str("## Proposals\n\n");
            for p in proposals {
                s.push_str(&p);
            }
        }

        let goals = goal_lines(store, id, &rids);
        if !goals.is_empty() {
            s.push_str("## Goals\n\n");
            s.push_str(&goals.join("\n"));
            s.push_str("\n\n");
        }

        let diags: Vec<String> = store
            .graph
            .diagnostics
            .iter()
            .filter(|(_, d)| {
                d.lifecycle == "open"
                    && d.subjects.iter().any(|sj| {
                        let resolved = store.resolve_id(sj);
                        resolved == id.as_str() || rids.iter().any(|r| r == resolved)
                    })
            })
            .map(|(did, d)| {
                let triage = d
                    .triage
                    .as_ref()
                    .map(|t| format!(" · triage: {}", t))
                    .unwrap_or_default();
                format!(
                    "- `{}` [{}] {}: {}{}",
                    did, d.severity, d.rule, d.message, triage
                )
            })
            .collect();
        if !diags.is_empty() {
            s.push_str("## Open diagnostics\n\n");
            s.push_str(&diags.join("\n"));
            s.push_str("\n\n");
        }

        if !ent.mentions.is_empty() {
            s.push_str("## Mentioned in\n\n");
            for m in &ent.mentions {
                s.push_str(&format!(
                    "- `{}#{}`: \"{}\"\n",
                    m.doc,
                    m.section,
                    m.quote.split_whitespace().collect::<Vec<_>>().join(" ")
                ));
            }
        }
        std::fs::write(dir.join(format!("{}.md", slug(id))), s).ok();
        written += 1;
    }

    // The index: every entity linked, the default class and component views rendered
    // from the graph, every view listed, and the pending proposals grouped by target
    // document. Mirrors docs/consumers/docsgen.md#relationships-view.
    if !store.graph.entities.is_empty() {
        let mut idx = String::new();
        idx.push_str("# Index\n\n");
        for (id, ent) in &store.graph.entities {
            idx.push_str(&format!("- [{}](./{}.md) `{}`\n", ent.name, slug(id), id));
        }

        let class_views: Vec<(&String, &View)> = store
            .graph
            .views
            .iter()
            .filter(|(vid, _)| vid.starts_with("view:class/"))
            .collect();
        let component_views: Vec<(&String, &View)> = store
            .graph
            .views
            .iter()
            .filter(|(vid, _)| vid.starts_with("view:component/"))
            .collect();
        if !class_views.is_empty() || !component_views.is_empty() {
            idx.push_str("\n## Diagrams\n\n");
            for (vid, v) in class_views.iter().chain(component_views.iter()) {
                idx.push_str(&embed(store, vid, v));
            }
        }

        let mut all_views: Vec<(String, View)> = store
            .graph
            .views
            .iter()
            .map(|(vid, v)| (vid.clone(), v.clone()))
            .collect();
        for m in store.graph.state_machines.values() {
            let vid = format!("view:state/{}", entity_slug(&m.subject));
            if !store.graph.views.contains_key(&vid) {
                if let Some(v) = synthesized_state_view(store, &m.subject) {
                    all_views.push((vid, v));
                }
            }
        }
        all_views.sort_by(|a, b| a.0.cmp(&b.0));
        if !all_views.is_empty() {
            idx.push_str("## Views\n\n");
            for (vid, v) in &all_views {
                let (svg, puml) = diagram_rel(vid).unwrap_or_default();
                let mut line = format!(
                    "- `{}` ({}) {} · {} members · [svg]({}) · [puml]({})",
                    vid,
                    v.kind,
                    v.title,
                    v.members.len(),
                    svg,
                    puml
                );
                if let Some(goal) = over_limit_goal(store, vid, v) {
                    line.push_str(&format!(" · goal `{}`", goal));
                }
                idx.push_str(&line);
                idx.push('\n');
            }
            idx.push('\n');
        }

        // The ratification report: every pending proposal, grouped by the document
        // its edit targets, so an owner reviews one document's proposals together.
        let mut by_doc: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (did, d) in &store.graph.diagnostics {
            if d.lifecycle != "open"
                || (d.rule != "ratification-pending" && d.rule != "invented-choice")
            {
                continue;
            }
            let Some(p) = &d.prompt else { continue };
            let edit = p.options.iter().find_map(|o| o.edit.as_ref());
            let (doc, line) = match edit {
                Some(e) => (
                    e.doc.clone(),
                    format!(
                        "- `{}` ({}): {} → `{}#{}`",
                        did,
                        d.subjects.join(", "),
                        e.new_text,
                        e.doc,
                        e.section
                    ),
                ),
                None => (
                    "(no target)".to_string(),
                    format!("- `{}` ({}): {}", did, d.subjects.join(", "), d.message),
                ),
            };
            by_doc.entry(doc).or_default().push(line);
        }
        if !by_doc.is_empty() {
            idx.push_str("## Ratification\n\n");
            for (doc, lines) in &by_doc {
                idx.push_str(&format!("### `{}`\n\n", doc));
                idx.push_str(&lines.join("\n"));
                idx.push_str("\n\n");
            }
        }
        std::fs::write(dir.join("index.md"), idx).ok();
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn gs(out: &std::path::Path) -> GenSettings {
        GenSettings {
            deliverable: out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        }
    }

    #[test]
    fn renders_and_prunes() {
        let out = std::env::temp_dir().join(format!("jazyk-docsgen-test-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let mut s = Store {
            out: out.clone(),
            ..Default::default()
        };
        s.graph.entities.insert(
            "ent:cart".into(),
            Entity {
                name: "Cart".into(),
                definition: Some("holds items".into()),
                mentions: vec![SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop".into(),
                    quote: "the Cart".into(),
                }],
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:shop-1".into(),
            Requirement {
                statement: "The Cart shall hold items.".into(),
                entities: vec!["ent:cart".into()],
                edges: vec![],
                source: Some(SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop".into(),
                    quote: "holds\nitems".into(),
                }),
                ..Default::default()
            },
        );
        // A stale file for an entity that no longer exists must be pruned.
        std::fs::create_dir_all(out.join("docsgen")).ok();
        std::fs::write(out.join("docsgen/ghost.md"), "old").ok();
        let n = write_all(&s, &gs(&out));
        assert_eq!(n, 1);
        let doc = std::fs::read_to_string(out.join("docsgen/cart.md")).unwrap();
        assert!(doc.contains("# Cart"));
        assert!(doc.contains("req:shop-1"));
        assert!(
            doc.contains("> holds items"),
            "quote is whitespace-normalized: {}",
            doc
        );
        assert!(!out.join("docsgen/ghost.md").exists());
    }

    // The showcase graph: an entity page embeds the scope class view, the containing
    // system's component view, its own state view, and the flow view naming it, each
    // as a relative image link into ../diagrams/, with captions cross-linking the
    // member pages. Mirrors docs/consumers/docsgen.md#diagrams-on-entity-pages.
    #[test]
    fn showcase_page_embeds_images_and_cross_links() {
        let out =
            std::env::temp_dir().join(format!("jazyk-docsgen-showcase-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        std::fs::create_dir_all(&out).unwrap();
        let mut s = crate::derive::tests::showcase_store();
        s.out = out.clone();
        let mut batch = crate::store::RecordBatch::new(1);
        crate::derive::recompute(&mut s, "g1", &mut batch);
        write_all(&s, &gs(&out));

        let page = std::fs::read_to_string(out.join("docsgen/order.md")).unwrap();
        // The images, in the stated order: the level neighborhood (the parent's level
        // view, order-service holding four children; the order itself holds none), own
        // state view, the flow view naming the order. The scope root's level view and
        // the shop's belong to the pages of their members, not to the order's.
        let level = crate::derive::level_view_id(&s, "ent:order-service").unwrap();
        let level_svg = format!("](../diagrams/{}.svg)", &level["view:".len()..]);
        assert!(page.contains(&level_svg), "{}", page);
        assert!(
            !page.contains("](../diagrams/component/public.svg)"),
            "{}",
            page
        );
        assert!(
            !page.contains("](../diagrams/component/shop.svg)"),
            "{}",
            page
        );
        assert!(page.contains("](../diagrams/state/order.svg)"), "{}", page);
        assert!(
            page.contains("](../diagrams/usecase/customer-shop.svg)"),
            "{}",
            page
        );
        let level_pos = page.find(&level_svg).unwrap();
        let state_pos = page.find("../diagrams/state/order.svg").unwrap();
        let flow_pos = page.find("../diagrams/usecase/customer-shop.svg").unwrap();
        assert!(level_pos < state_pos && state_pos < flow_pos);
        // The state caption lists the states; the level caption cross-links members.
        assert!(page.contains("3 states: placed, paid, held"), "{}", page);
        assert!(page.contains("(./checkout-api.md)"), "{}", page);
        assert!(
            page.contains(&format!(
                "[source](../diagrams/{}.puml)",
                &level["view:".len()..]
            )),
            "{}",
            page
        );
        // The rendered files exist, so the links resolve.
        assert!(out.join("diagrams/state/order.svg").exists());

        // The index embeds the structural level views as images (no mermaid), the
        // scope root's (view:component/public: shop is a system, customer an actor)
        // and the shop's, lists every view, and cross-links the pages.
        let idx = std::fs::read_to_string(out.join("docsgen/index.md")).unwrap();
        assert!(
            idx.contains("](../diagrams/component/public.svg)"),
            "{}",
            idx
        );
        assert!(!idx.contains("](../diagrams/class/public.svg)"), "{}", idx);
        assert!(idx.contains("](../diagrams/component/shop.svg)"), "{}", idx);
        assert!(!idx.contains("mermaid"), "{}", idx);
        assert!(idx.contains("- [Order](./order.md) `ent:order`"), "{}", idx);
        assert!(idx.contains("`view:usecase/customer-shop`"), "{}", idx);
        std::fs::remove_dir_all(&out).ok();
    }

    // A pending ratification proposal renders as its prompt on the entity page, and
    // the index groups proposals by target document.
    // Mirrors docs/consumers/docsgen.md#ratification-proposals.
    #[test]
    fn ratification_proposal_renders_as_a_prompt() {
        let out = std::env::temp_dir().join(format!("jazyk-docsgen-ratify-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let mut s = Store {
            out: out.clone(),
            ..Default::default()
        };
        s.graph.entities.insert(
            "ent:pricing".into(),
            Entity {
                name: "Pricing".into(),
                provenance: Some(Provenance::Derived {
                    from: vec!["req:orders-3".into()],
                    reasoning: "a separable concern".into(),
                }),
                ..Default::default()
            },
        );
        s.graph.diagnostics.insert(
            "diag:ratification-pending-1".into(),
            Diagnostic {
                rule: "ratification-pending".into(),
                severity: "warning".into(),
                subjects: vec!["ent:pricing".into()],
                message: "ent:pricing is derived and no document states it.".into(),
                reasoning: Some("req:orders-3 separates pricing".into()),
                lifecycle: "open".into(),
                triage: None,
                prompt: Some(DiagnosticPrompt {
                    question: "Should docs/orders.md state the pricing module?".into(),
                    options: vec![
                        PromptOption {
                            label: "Insert into docs/orders.md /orders/service".into(),
                            edit: Some(SuggestedEdit {
                                doc: "docs/orders.md".into(),
                                section: "/orders/service".into(),
                                old_text: String::new(),
                                new_text: "The order service contains a pricing module.".into(),
                            }),
                            answer: None,
                        },
                        PromptOption {
                            label: "Retract".into(),
                            edit: None,
                            answer: Some("retract".into()),
                        },
                    ],
                    freeform: true,
                }),
                answer: None,
                created: None,
                updated: None,
            },
        );
        write_all(&s, &gs(&out));
        let page = std::fs::read_to_string(out.join("docsgen/pricing.md")).unwrap();
        assert!(page.contains("## Proposals"), "{}", page);
        assert!(
            page.contains("> The order service contains a pricing module."),
            "{}",
            page
        );
        assert!(
            page.contains("Target: `docs/orders.md#/orders/service`"),
            "{}",
            page
        );
        assert!(page.contains("- Answer: Retract"), "{}", page);
        assert!(
            page.contains("This entity is derived"),
            "the header names the provenance: {}",
            page
        );
        let idx = std::fs::read_to_string(out.join("docsgen/index.md")).unwrap();
        assert!(idx.contains("## Ratification"), "{}", idx);
        assert!(idx.contains("### `docs/orders.md`"), "{}", idx);
        std::fs::remove_dir_all(&out).ok();
    }
}
