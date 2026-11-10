use super::documents::synthesized_state_view;
use super::levels::{level_link, level_page, level_targets};
use super::links::{
    card_path, diagram_rel, embed, github_anchor, over_limit_goal, rel, source_prefix, DOCS_DIR,
};
use crate::ids::entity_slug;
use crate::model::View;
use crate::store::Store;
use std::collections::{BTreeMap, BTreeSet};

// Writes the index: every entity linked, the default class and component views
// rendered from the graph, every view listed, and the pending proposals grouped by
// target document. Mirrors docs/consumers/docsgen.md#relationships-view.
pub(super) fn write_index(store: &Store, dir: &std::path::Path) {
    let mut idx = String::new();
    idx.push_str("# Index\n\n");
    // The walk first: per scope, the level page of the root and its members as
    // cards. Mirrors docs/consumers/docsgen.md#relationships-view.
    let mut scopes: Vec<String> = store
        .graph
        .entities
        .values()
        .map(|e| e.scope.clone())
        .collect();
    scopes.sort();
    scopes.dedup();
    for scope in &scopes {
        let target = format!("{}{}", crate::board::SCOPE_TARGET_PREFIX, scope);
        let members: Vec<String> = crate::goals::level_members(store, &target)
            .iter()
            .map(|m| {
                let n = crate::goals::level_members(store, m).len();
                let link = format!(
                    "[{}]({})",
                    store.graph.entities[m].name,
                    rel(DOCS_DIR, &card_path(m))
                );
                if n >= 2 {
                    format!("{} ({} children)", link, n)
                } else {
                    link
                }
            })
            .collect();
        // The level page exists only for a root with two or more members; a
        // one-member scope starts at the member's card.
        // Mirrors docs/consumers/docsgen.md#relationships-view.
        let page = if level_targets(store).iter().any(|t| t == &target) {
            format!(", [level page](./{})", level_page(&target))
        } else {
            String::new()
        };
        idx.push_str(&format!(
            "Start here: the top level of `{}`{}, then {}\n\n",
            scope,
            page,
            if members.is_empty() {
                "nothing yet".to_string()
            } else {
                members.join(", ")
            }
        ));
    }
    for (id, ent) in &store.graph.entities {
        idx.push_str(&format!(
            "- [{}](./{}.md) `{}`\n",
            ent.name,
            entity_slug(id),
            id
        ));
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
    // The scope root's level view is the index's picture of a scope, and its
    // caption links to the scope root's level page, the top of the level pages.
    // Mirrors docs/consumers/docsgen.md#relationships-view.
    let mut root_levels: BTreeMap<String, String> = BTreeMap::new();
    for t in level_targets(store) {
        if let Some(scope) = crate::board::scope_target(&t) {
            if let Some(vid) = crate::derive::level_view_id(store, &t) {
                root_levels.insert(vid, scope.to_string());
            }
        }
    }
    let mut linked_scopes: BTreeSet<String> = BTreeSet::new();
    if !class_views.is_empty() || !component_views.is_empty() {
        idx.push_str("\n## Diagrams\n\n");
        // The scope roots' level views first, then the deeper levels: the index
        // opens on the top of each scope. Mirrors docs/consumers/docsgen.md#relationships-view.
        let mut ordered: Vec<(&String, &View)> = class_views
            .iter()
            .chain(component_views.iter())
            .cloned()
            .collect();
        ordered.sort_by_key(|(vid, _)| (!root_levels.contains_key(vid.as_str()), vid.to_string()));
        for (vid, v) in ordered {
            let mut block = embed(store, vid, v, DOCS_DIR);
            if let Some(scope) = root_levels.get(vid.as_str()) {
                let target = crate::store::scope_root_target(scope);
                if let Some(link) = level_link(store, &target, DOCS_DIR) {
                    let trimmed = block.trim_end().len();
                    block.truncate(trimmed);
                    block.push_str(&format!(" · level page: {}\n\n", link));
                    linked_scopes.insert(scope.clone());
                }
            }
            idx.push_str(&block);
        }
    }
    // A scope root whose level view is not stored yet (the next commit derives
    // it) still gets its level page linked, so the top of the levels is always
    // reachable from the index.
    for scope in root_levels.values() {
        if linked_scopes.contains(scope) {
            continue;
        }
        let target = crate::store::scope_root_target(scope);
        if let Some(link) = level_link(store, &target, DOCS_DIR) {
            idx.push_str(&format!("Level page of scope `{}`: {}\n\n", scope, link));
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
    idx.push_str("Reports: [glossary](./glossary.md) · [fragmentation](./fragmentation.md) · [staleness](./staleness.md)\n\n");
    crate::vfs::write(dir.join("index.md"), idx).ok();
}

// The three reports the index links: the glossary, the fragmentation report, and the
// staleness report. Mirrors docs/consumers/docsgen.md#glossary,
// docs/consumers/docsgen.md#fragmentation-reports, and
// docs/consumers/docsgen.md#staleness-reports.
pub(super) fn write_reports(store: &Store, dir: &std::path::Path) -> usize {
    // A link into a source document: relative from docsgen when the out directory
    // sits where the default puts it, the bare reference otherwise.
    let prefix = source_prefix(store);
    let section_link = |doc: &str, section: &str| -> String {
        let text = format!("{}#{}", doc, section);
        if let Some(prefix) = prefix {
            // A GitHub anchor is the heading text alone, never the path.
            let title = store
                .docs
                .get(doc)
                .and_then(|d| d.sections.get(section))
                .map(|s| s.title.clone())
                .unwrap_or_else(|| section.rsplit('/').next().unwrap_or(section).to_string());
            format!("[{}]({}{}#{})", text, prefix, doc, github_anchor(&title))
        } else {
            format!("`{}`", text)
        }
    };
    // Glossary.
    let mut g = String::from("# Glossary\n\n");
    let mut entities: Vec<(&String, &crate::model::Entity)> = store.graph.entities.iter().collect();
    entities.sort_by(|a, b| {
        a.1.name
            .to_lowercase()
            .cmp(&b.1.name.to_lowercase())
            .then(a.0.cmp(b.0))
    });
    for (id, e) in &entities {
        let mut line = format!("- **{}**", e.name);
        if !e.aliases.is_empty() {
            line.push_str(&format!(" (also: {})", e.aliases.join(", ")));
        }
        if let Some(st) = &e.stereotype {
            line.push_str(&format!(" «{}»", st));
        }
        if let Some(d) = &e.definition {
            line.push_str(&format!(": {}", d));
        }
        line.push_str(&format!(" · [card]({})", rel(DOCS_DIR, &card_path(id))));
        let mut secs: Vec<(String, String)> = e
            .mentions
            .iter()
            .map(|m| (m.doc.clone(), m.section.clone()))
            .collect();
        secs.sort();
        secs.dedup();
        if !secs.is_empty() {
            let links: Vec<String> = secs.iter().map(|(d, sct)| section_link(d, sct)).collect();
            line.push_str(&format!(" · sections: {}", links.join(", ")));
        }
        g.push_str(&line);
        g.push('\n');
    }
    crate::vfs::write(dir.join("glossary.md"), g).ok();
    // Fragmentation.
    let mut rows: Vec<(usize, usize, String, String, Vec<String>)> = Vec::new();
    for (id, e) in &store.graph.entities {
        let mut docs: Vec<String> = e.mentions.iter().map(|m| m.doc.clone()).collect();
        docs.sort();
        docs.dedup();
        let mut secs: Vec<(String, String)> = e
            .mentions
            .iter()
            .map(|m| (m.doc.clone(), m.section.clone()))
            .collect();
        secs.sort();
        secs.dedup();
        if docs.len() > 1 {
            rows.push((docs.len(), secs.len(), e.name.clone(), id.clone(), docs));
        }
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    let mut f = String::from(
        "# Fragmentation\n\nEntities whose mentions span several documents, most spread first.\n\n",
    );
    if rows.is_empty() {
        f.push_str("No entity is mentioned in more than one document.\n");
    }
    for (nd, ns, name, id, docs) in &rows {
        f.push_str(&format!(
            "- [{}]({}): {} documents, {} sections: {}\n",
            name,
            rel(DOCS_DIR, &card_path(id)),
            nd,
            ns,
            docs.join(", ")
        ));
    }
    crate::vfs::write(dir.join("fragmentation.md"), f).ok();
    // Staleness.
    let anchor_of = |subject: &str| -> Option<(String, String)> {
        let rs = store.resolve_id(subject);
        if let Some(r) = store.graph.requirements.get(rs) {
            return r
                .source
                .as_ref()
                .map(|src| (src.doc.clone(), src.section.clone()));
        }
        store.graph.entities.get(rs).and_then(|e| {
            e.mentions
                .first()
                .map(|m| (m.doc.clone(), m.section.clone()))
        })
    };
    let mut per: BTreeMap<(String, String), BTreeMap<String, usize>> = BTreeMap::new();
    for d in store.graph.diagnostics.values() {
        if d.lifecycle != "open" {
            continue;
        }
        let mut anchors: Vec<(String, String)> =
            d.subjects.iter().filter_map(|s| anchor_of(s)).collect();
        anchors.sort();
        anchors.dedup();
        for a in anchors {
            *per.entry(a).or_default().entry(d.rule.clone()).or_insert(0) += 1;
        }
    }
    let mut ranked: Vec<(usize, (String, String), BTreeMap<String, usize>)> = per
        .into_iter()
        .map(|(k, rules)| (rules.values().sum::<usize>(), k, rules))
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut st = String::from("# Staleness\n\nOpen diagnostics by the section their subjects anchor in, most burdened first.\n\n");
    if ranked.is_empty() {
        st.push_str("No open diagnostic anchors in a section.\n");
    }
    for (total, (doc, section), rules) in &ranked {
        let parts: Vec<String> = rules.iter().map(|(r, n)| format!("{} ×{}", r, n)).collect();
        st.push_str(&format!(
            "- {}: {} ({})\n",
            section_link(doc, section),
            total,
            parts.join(", ")
        ));
    }
    let mut weak: Vec<String> = Vec::new();
    for (doc, rec) in &store.docs {
        for (sec, cov) in &rec.coverage {
            if cov.state == "non-normative"
                && cov.note.as_deref().map(|n| n.trim().len()).unwrap_or(0) < 20
            {
                weak.push(format!(
                    "- {}: {}",
                    section_link(doc, sec),
                    cov.note
                        .as_deref()
                        .filter(|n| !n.trim().is_empty())
                        .unwrap_or("(no note)")
                ));
            }
        }
    }
    if !weak.is_empty() {
        st.push_str("\n## Non-normative sections to re-review\n\nMarked non-normative with an empty or short note.\n\n");
        st.push_str(&weak.join("\n"));
        st.push('\n');
    }
    crate::vfs::write(dir.join("staleness.md"), st).ok();
    3
}
