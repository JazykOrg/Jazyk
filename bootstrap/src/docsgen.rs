// Deterministic per-entity requirements documents: the reading surface between prose
// and graph. Rendered on every converged build, no LLM. Mirrors
// docs/consumers/docsgen.md#the-requirements-document.
use crate::gen::GenSettings;
use crate::store::Store;
use std::collections::BTreeSet;

fn slug(id: &str) -> String {
    id.strip_prefix("ent:").unwrap_or(id).to_string()
}

pub fn write_all(store: &Store, gs: &GenSettings) -> usize {
    let vmap = crate::verify::status_map(store, gs);
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

        let mut rids = store.requirements_referencing(id);
        rids.sort();
        if !rids.is_empty() {
            s.push_str("## Requirements\n\n");
            for rid in &rids {
                let Some(r) = store.graph.requirements.get(rid) else { continue };
                s.push_str(&format!("### `{}`\n\n{}\n\n", rid, r.ears));
                s.push_str(&format!(
                    "> {}\n\nSource: `{}#{}`",
                    r.source.quote.split_whitespace().collect::<Vec<_>>().join(" "),
                    r.source.doc,
                    r.source.section
                ));
                let others: Vec<&str> = r
                    .entities
                    .iter()
                    .filter(|e| store.resolve_id(e) != id.as_str())
                    .map(|e| e.as_str())
                    .collect();
                if !others.is_empty() {
                    s.push_str(&format!(" · ties: {}", others.join(", ")));
                }
                s.push_str("\n\n");
                if let Some(v) = vmap.get(rid.as_str()) {
                    let status = v["status"].as_str().unwrap_or("missing");
                    let mut line = format!("Verification: `{}`", status);
                    if let Some(name) = v["name"].as_str() {
                        line.push_str(&format!(" by `{}` ({})", name, v["kind"].as_str().unwrap_or("?")));
                    }
                    if let Some(t) = v["lastRun"].as_str() {
                        line.push_str(&format!(", last run {}", t));
                    }
                    if let Some(ev) = v["evidence"].as_str() {
                        line.push_str(&format!("\n\n> {}", ev.split_whitespace().collect::<Vec<_>>().join(" ")));
                    }
                    s.push_str(&line);
                    s.push_str("\n\n");
                }
            }
        }

        let rels: Vec<String> = store
            .graph
            .relationships
            .iter()
            .filter(|(_, rel)| rel.members.contains(id))
            .map(|(rid, rel)| {
                let other = rel.members.iter().find(|m| *m != id).cloned().unwrap_or_default();
                format!("- `{}` {} `{}` (from {})", rid, rel.rel_type, other, rel.requirements.join(", "))
            })
            .collect();
        if !rels.is_empty() {
            s.push_str("## Relationships\n\n");
            s.push_str(&rels.join("\n"));
            s.push_str("\n\n");
        }

        let diags: Vec<String> = store
            .graph
            .diagnostics
            .iter()
            .filter(|(_, d)| {
                d.lifecycle == "open" && d.subjects.iter().any(|sj| store.resolve_id(sj) == id.as_str())
            })
            .map(|(did, d)| format!("- `{}` [{}] {}: {}", did, d.severity, d.rule, d.message))
            .collect();
        if !diags.is_empty() {
            s.push_str("## Open diagnostics\n\n");
            s.push_str(&diags.join("\n"));
            s.push_str("\n\n");
        }

        if !ent.mentions.is_empty() {
            s.push_str("## Mentioned in\n\n");
            for m in &ent.mentions {
                s.push_str(&format!("- `{}#{}`\n", m.doc, m.section));
            }
        }
        std::fs::write(dir.join(format!("{}.md", slug(id))), s).ok();
        written += 1;
    }

    // The index: one entry per entity plus a relationships view rendered from the
    // graph, so the diagram cannot drift the way a hand-drawn one does.
    // Mirrors docs/consumers/docsgen.md#relationships-view.
    if !store.graph.entities.is_empty() {
        let mid = |id: &str| id.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
        let mut idx = String::new();
        idx.push_str("# Index\n\n");
        for (id, ent) in &store.graph.entities {
            idx.push_str(&format!("- [{}](./{}.md) `{}`\n", ent.name, slug(id), id));
        }
        if !store.graph.relationships.is_empty() {
            idx.push_str("\n## Relationships\n\n");
            if store.graph.entities.len() > 60 {
                idx.push_str("(the graph is past a readable diagram; see each entity's Relationships section)\n");
            } else {
                idx.push_str("```mermaid\nflowchart LR\n");
                let mut named: BTreeSet<String> = BTreeSet::new();
                let mut edges: BTreeSet<(String, String, String)> = BTreeSet::new();
                for rel in store.graph.relationships.values() {
                    if rel.members.len() < 2 {
                        continue;
                    }
                    let (a, b) = (rel.members[0].clone(), rel.members[1].clone());
                    for m in [&a, &b] {
                        if named.insert(m.clone()) {
                            let name = store
                                .graph
                                .entities
                                .get(store.resolve_id(m))
                                .map(|e| e.name.replace('"', "'"))
                                .unwrap_or_else(|| m.clone());
                            idx.push_str(&format!("    {}[\"{}\"]\n", mid(m), name));
                        }
                    }
                    if edges.insert((a.clone(), b.clone(), rel.rel_type.clone())) {
                        idx.push_str(&format!("    {} -->|{}| {}\n", mid(&a), rel.rel_type, mid(&b)));
                    }
                }
                idx.push_str("```\n");
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

    #[test]
    fn renders_and_prunes() {
        let out = std::env::temp_dir().join(format!("jazyk-docsgen-test-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let mut s = Store { out: out.clone(), ..Default::default() };
        s.graph.entities.insert(
            "ent:cart".into(),
            Entity {
                name: "Cart".into(),
                definition: Some("holds items".into()),
                mentions: vec![SourceRef { doc: "shop.md".into(), section: "/shop".into(), quote: "the Cart".into() }],
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:shop-1".into(),
            Requirement {
                ears: "The Cart shall hold items.".into(),
                entities: vec!["ent:cart".into()],
                edges: vec![],
                source: SourceRef { doc: "shop.md".into(), section: "/shop".into(), quote: "holds\nitems".into() },
                confidence: None,
                reasoning: None,
                created: None,
                updated: None,
            },
        );
        // A stale file for an entity that no longer exists must be pruned.
        std::fs::create_dir_all(out.join("docsgen")).ok();
        std::fs::write(out.join("docsgen/ghost.md"), "old").ok();
        let n = write_all(&s, &GenSettings { deliverable: out.join("product"), worker: "agentic".into(), code: Vec::new() });
        assert_eq!(n, 1);
        let doc = std::fs::read_to_string(out.join("docsgen/cart.md")).unwrap();
        assert!(doc.contains("# Cart"));
        assert!(doc.contains("req:shop-1"));
        assert!(doc.contains("> holds items"), "quote is whitespace-normalized: {}", doc);
        assert!(!out.join("docsgen/ghost.md").exists());
    }
}
