// Requirements documents: one per entity, the reading surface closest to the prose.
// Mirrors docs/consumers/docsgen.md#the-requirements-document.
use super::levels::level_link;
use super::links::{
    card_path, embed, page_link, rel, relink_quote, view_page_path, DOCS_DIR, FLOW_KINDS,
};
use crate::ids::entity_slug;
use crate::model::{Diagnostic, Goal, Provenance, View};
use crate::store::Store;
use std::collections::{BTreeMap, BTreeSet};

// A markdown link, its text in the first group.
static MARKDOWN_LINK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"\[([^\]]*)\]\([^)]*\)").expect("a literal pattern")
});

// The synthesized state view of an entity's machine, when no stored view exists (the
// renderer synthesizes the same one). Mirrors render targets.
pub(super) fn synthesized_state_view(store: &Store, id: &str) -> Option<View> {
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

pub(super) fn prov_short(p: &Provenance) -> String {
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
    for p in &store.status.parked {
        if matches(&p.goal) {
            let why = p
                .reason
                .as_deref()
                .map(|r| format!(": {}", r))
                .unwrap_or_default();
            out.push(format!(
                "- `{}` ({}) parked{}{}",
                p.goal.id,
                p.goal.kind,
                why,
                cause(&p.goal)
            ));
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

// Writes one requirements document per entity and removes the documents of absent
// entities. Returns the documents written.
pub(super) fn write_documents(
    store: &Store,
    dir: &std::path::Path,
    instances: &BTreeMap<String, String>,
    ledger: &crate::gen::Ledger,
    vmap: &BTreeMap<String, serde_json::Value>,
) -> usize {
    // Stale documents for absent entities are removed, so links never mislead.
    let live: BTreeSet<String> = store
        .graph
        .entities
        .keys()
        .map(|id| entity_slug(id).to_string())
        .collect();
    if let Ok(rd) = crate::vfs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(stem) = name.strip_suffix(".md") {
                if stem != "index" && !live.contains(stem) {
                    crate::vfs::remove_file(e.path()).ok();
                }
            }
        }
    }
    let mut written = 0;
    for (id, ent) in &store.graph.entities {
        let mut s = String::new();
        s.push_str(&format!("# {}\n\n", ent.name));
        s.push_str(&format!("`{}`", id));
        if !ent.aliases.is_empty() {
            s.push_str(&format!(" · also known as: {}", ent.aliases.join(", ")));
        }
        if let Some(st) = &ent.stereotype {
            s.push_str(&format!(" · «{}»", st));
        }
        if ent.scope != "public" {
            s.push_str(&format!(" · scope `{}`", ent.scope));
        }
        // The one link out of the documents' web: the card, where the walk starts.
        s.push_str(&format!(" · [card]({})", rel(DOCS_DIR, &card_path(id))));
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
        // The level pages: the parent's level (the scope root's for a parentless
        // entity) and the entity's own when it has one.
        {
            let parent_target = match &ent.parent {
                Some(p) => store.resolve_id(p).to_string(),
                None => format!("{}{}", crate::board::SCOPE_TARGET_PREFIX, ent.scope),
            };
            let mut parts: Vec<String> = Vec::new();
            if let Some(l) = level_link(store, &parent_target, DOCS_DIR) {
                parts.push(format!("Level: {}", l));
            }
            if let Some(l) = level_link(store, id, DOCS_DIR) {
                parts.push(format!("Own level: {}", l));
            }
            if !parts.is_empty() {
                s.push_str(&parts.join(" · "));
                s.push_str("\n\n");
            }
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
        // Flows are listed, never embedded: an entity in five flows would open with
        // eleven pictures. Mirrors docs/consumers/docsgen.md#diagrams-on-entity-pages.
        let views = relevant_views(store, id, instances);
        let (flows, pictures): (Vec<_>, Vec<_>) = views
            .into_iter()
            .partition(|(_, v)| FLOW_KINDS.contains(&v.kind.as_str()));
        if !pictures.is_empty() {
            s.push_str("## Diagrams\n\n");
            for (vid, v) in &pictures {
                s.push_str(&embed(store, vid, v, DOCS_DIR));
            }
        }
        if !flows.is_empty() {
            s.push_str("## Flows\n\n");
            for (vid, v) in &flows {
                let page = view_page_path(vid)
                    .map(|p| rel(DOCS_DIR, &p))
                    .unwrap_or_default();
                s.push_str(&format!(
                    "- {} · {} · {} steps · [page]({}) `{}`\n",
                    v.title,
                    v.kind,
                    v.members.len(),
                    page,
                    vid
                ));
            }
            s.push('\n');
        }

        let mut rids = store.requirements_referencing(id);
        rids.sort();
        if !rids.is_empty() {
            s.push_str("## Requirements\n\n");
            // Grouped by source document and section in document order, derived and
            // decreed last; an index opens a long section
            // (docs/consumers/docsgen.md#the-requirements-document).
            let group_key = |rid: &String| -> (usize, String, String) {
                match store
                    .graph
                    .requirements
                    .get(rid)
                    .and_then(|r| r.source.as_ref())
                {
                    Some(src) => {
                        let order = store
                            .docs
                            .get(&src.doc)
                            .map(|rec| {
                                rec.sections
                                    .keys()
                                    .position(|k| k == &src.section)
                                    .unwrap_or(usize::MAX)
                            })
                            .unwrap_or(usize::MAX);
                        (0, src.doc.clone(), format!("{:08}{}", order, src.section))
                    }
                    None => (1, String::new(), String::new()),
                }
            };
            rids.sort_by(|a, b| group_key(a).cmp(&group_key(b)).then(a.cmp(b)));
            // One line per source group: where the statements come from, before
            // they come (docs/consumers/docsgen.md#the-requirements-document).
            if rids.len() > 5 {
                let mut groups: Vec<(String, String, usize)> = Vec::new();
                for rid in &rids {
                    let Some(r) = store.graph.requirements.get(rid) else {
                        continue;
                    };
                    let label = match r.source.as_ref() {
                        Some(src) => {
                            let t = store
                                .docs
                                .get(&src.doc)
                                .and_then(|rec| rec.sections.get(&src.section))
                                .map(|sec| sec.title.clone())
                                .unwrap_or_default();
                            if t.is_empty() {
                                src.doc.clone()
                            } else {
                                format!("{} § {}", src.doc, t)
                            }
                        }
                        None => "Derived and decreed".to_string(),
                    };
                    match groups.last_mut() {
                        Some((l, _, n)) if *l == label => *n += 1,
                        _ => groups.push((label, rid.replace(':', ""), 1)),
                    }
                }
                for (label, anchor, n) in &groups {
                    s.push_str(&format!(
                        "- [{}](#{}): {} statement{}\n",
                        label,
                        anchor,
                        n,
                        if *n == 1 { "" } else { "s" }
                    ));
                }
                s.push('\n');
            }
            let mut last_group: Option<(String, String)> = None;
            for rid in &rids {
                let Some(r) = store.graph.requirements.get(rid) else {
                    continue;
                };
                let group = match r.source.as_ref() {
                    Some(src) => (src.doc.clone(), src.section.clone()),
                    None => (String::new(), String::new()),
                };
                if last_group.as_ref() != Some(&group) {
                    let title = match r.source.as_ref() {
                        Some(src) => {
                            let t = store
                                .docs
                                .get(&src.doc)
                                .and_then(|rec| rec.sections.get(&src.section))
                                .map(|sec| sec.title.clone())
                                .unwrap_or_default();
                            if t.is_empty() {
                                format!("**{}**", src.doc)
                            } else {
                                format!("**{} § {}**", src.doc, t)
                            }
                        }
                        None => "**Derived and decreed**".to_string(),
                    };
                    s.push_str(&format!("{}\n\n", title));
                    last_group = Some(group);
                }
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
                // The quote says what the statement says once case, punctuation,
                // link targets, articles, modal verbs, and a trailing `s` are set aside
                // (docs/consumers/docsgen.md#the-requirements-document).
                let same = |a: &str, b: &str| {
                    let norm = |s: &str| -> Vec<String> {
                        let unlinked = MARKDOWN_LINK.replace_all(s, "$1").to_string();
                        unlinked
                            .to_lowercase()
                            .split(|c: char| !(c.is_alphanumeric() || c == '`'))
                            .map(|w| if w == "cannot" { "not" } else { w })
                            .filter(|w| {
                                !w.is_empty()
                                    && ![
                                        "shall", "must", "will", "should", "can", "then", "the",
                                        "a", "an",
                                    ]
                                    .contains(w)
                            })
                            .map(|w| {
                                if w.len() > 4 && w.ends_with("ies") {
                                    format!("{}y", &w[..w.len() - 3])
                                } else if w.len() > 3 && w.ends_with('s') {
                                    w[..w.len() - 1].to_string()
                                } else {
                                    w.to_string()
                                }
                            })
                            .collect()
                    };
                    norm(a) == norm(b)
                };
                match r.source.as_ref() {
                    // The quote is omitted when it equals the statement: a page once
                    // repeated every statement twice.
                    Some(src) if same(&src.quote, &r.statement) => {
                        s.push_str(&format!("Source: `{}#{}`", src.doc, src.section))
                    }
                    Some(src) => s.push_str(&format!(
                        "> {}\n\nSource: `{}#{}`",
                        relink_quote(
                            &src.quote.split_whitespace().collect::<Vec<_>>().join(" "),
                            &src.doc,
                            store
                        ),
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
                // The requirement's criteria with their test verdicts
                // (docs/consumers/docsgen.md#the-requirements-document).
                let crits = store.criteria_of(rid);
                if !crits.is_empty() {
                    s.push_str("Criteria:\n\n");
                    for cid in &crits {
                        if let Some(c) = store.graph.criteria.get(cid) {
                            let where_ = match &c.source {
                                Some(src) => format!("{}#{}", src.doc, src.section),
                                None => "proposed".to_string(),
                            };
                            let verdict = ledger
                                .requirements
                                .get(rid.as_str())
                                .and_then(|row| row.tests.get(cid))
                                .map(|t| format!(" · test `{}` {}", t.test.name, t.verdict))
                                .unwrap_or_default();
                            s.push_str(&format!(
                                "- `{}` [{}] {} ({}){}\n",
                                cid, c.kind, c.statement, where_, verdict
                            ));
                        }
                    }
                    s.push('\n');
                }
                // No row, no line (docs/consumers/docsgen.md#the-requirements-document).
                if let Some(v) = vmap
                    .get(rid.as_str())
                    .filter(|v| v["status"].as_str() != Some("missing"))
                {
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
                    relink_quote(
                        &m.quote.split_whitespace().collect::<Vec<_>>().join(" "),
                        &m.doc,
                        store
                    )
                ));
            }
        }
        crate::vfs::write(dir.join(format!("{}.md", entity_slug(id))), s).ok();
        written += 1;
    }
    written
}
