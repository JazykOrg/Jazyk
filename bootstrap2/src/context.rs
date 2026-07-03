// The context engine: assemble a bounded slice of the graph around a target.
// Pure computation, no LLM. Mirrors docs2/compiler/context.md.
use crate::model::split_section_ref;
use crate::store::Store;
use serde::Serialize;

#[derive(Clone, Copy)]
pub struct Focus {
    pub parents: u32,
    pub mentions: u32,
    pub requirements: u32,
}

impl Default for Focus {
    fn default() -> Self {
        Focus { parents: 2, mentions: 1, requirements: 2 }
    }
}

impl Focus {
    pub fn parse(s: &str) -> Focus {
        let mut f = Focus::default();
        for part in s.split(',') {
            if let Some((k, v)) = part.split_once('=') {
                if let Ok(n) = v.trim().parse::<u32>() {
                    match k.trim() {
                        "parents" => f.parents = n,
                        "mentions" => f.mentions = n,
                        "requirements" => f.requirements = n,
                        _ => {}
                    }
                }
            }
        }
        f
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Handle {
    pub handle: String,
    pub description: String,
    pub size: usize,
}

#[derive(Debug, Serialize)]
pub struct ContextPack {
    pub pack: String,
    pub handles: Vec<Handle>,
}

// Accumulates lines under a character budget; whatever does not fit becomes a handle.
struct Builder {
    budget: usize,
    text: String,
    handles: Vec<Handle>,
}

impl Builder {
    fn new(budget: usize) -> Builder {
        Builder { budget: budget.max(400), text: String::new(), handles: Vec::new() }
    }
    fn fits(&self, s: &str) -> bool {
        self.text.len() + s.len() <= self.budget
    }
    fn push(&mut self, s: &str) -> bool {
        if !self.fits(s) {
            return false;
        }
        self.text.push_str(s);
        self.text.push('\n');
        true
    }
    // Push a list of items under one axis; on overflow, emit a handle for the rest.
    fn push_items(&mut self, target: &str, axis: &str, start: usize, items: &[String], what: &str) {
        for (i, item) in items.iter().enumerate().skip(start) {
            if !self.push(item) {
                let remaining = items.len() - i;
                let size: usize = items[i..].iter().map(|s| s.len() + 1).sum();
                self.handles.push(Handle {
                    handle: format!("h:{}|{}|{}", target, axis, i),
                    description: format!("{} more {}", remaining, what),
                    size,
                });
                return;
            }
        }
    }
    fn finish(mut self) -> ContextPack {
        if !self.handles.is_empty() {
            self.text.push_str("\n### More\n");
            for h in &self.handles {
                self.text.push_str(&format!("- {} ({})\n", h.handle, h.description));
            }
        }
        ContextPack { pack: self.text, handles: self.handles }
    }
}

fn first_sentence(s: &str) -> String {
    let s = s.trim();
    match s.find(". ") {
        Some(i) => s[..=i].to_string(),
        None => crate::llm::truncate(s, 160),
    }
}

fn entity_line(store: &Store, id: &str) -> String {
    match store.graph.entities.get(id) {
        Some(e) => format!(
            "- {} ({}): {}",
            id,
            e.name,
            first_sentence(e.definition.as_deref().unwrap_or("(no definition yet)"))
        ),
        None => format!("- {} (unknown)", id),
    }
}

fn req_line(store: &Store, rid: &str, anchor_entity: Option<&str>) -> String {
    match store.graph.requirements.get(rid) {
        Some(r) => {
            let ties: Vec<&str> = r
                .entities
                .iter()
                .filter(|e| anchor_entity.map(|a| a != e.as_str()).unwrap_or(true))
                .map(|s| s.as_str())
                .collect();
            if ties.is_empty() {
                format!("- {}: {}", rid, r.ears)
            } else {
                format!("- {}: {} (ties: {})", rid, r.ears, ties.join(", "))
            }
        }
        None => format!("- {} (unknown)", rid),
    }
}

// Sorted requirement ids referencing an entity.
fn reqs_of(store: &Store, entity_id: &str) -> Vec<String> {
    let mut v = store.requirements_referencing(entity_id);
    v.sort();
    v
}

// Parent chain titles for a section, oldest first.
fn parent_chain(store: &Store, doc: &str, section: &str, hops: u32) -> Vec<String> {
    let mut chain = Vec::new();
    let Some(rec) = store.docs.get(doc) else { return chain };
    let mut cur = rec.sections.get(section).and_then(|s| s.parent.clone());
    for _ in 0..hops {
        match cur {
            Some(p) => {
                if let Some(sec) = rec.sections.get(&p) {
                    chain.push(format!("{}#{} ({})", doc, p, sec.title));
                    cur = sec.parent.clone();
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    chain.reverse();
    chain
}

// Assemble a context pack for a target: an entity id, a requirement id, a full section
// reference ("doc.md#/ref"), or a document path (its root section).
pub fn assemble(store: &Store, target: &str, focus: &Focus, budget: usize) -> Result<ContextPack, String> {
    let resolved = store.resolve_id(target).to_string();
    if resolved.starts_with("ent:") {
        return entity_pack(store, &resolved, focus, budget);
    }
    if resolved.starts_with("req:") {
        return req_pack(store, &resolved, focus, budget);
    }
    if let Some((doc, sec)) = split_section_ref(&resolved) {
        return section_pack(store, &doc, &sec, focus, budget);
    }
    if store.docs.contains_key(&resolved) {
        let root = store
            .docs
            .get(&resolved)
            .and_then(|d| d.sections.iter().find(|(_, s)| s.kind == "root").map(|(r, _)| r.clone()))
            .ok_or_else(|| format!("document {} has no root section", resolved))?;
        return section_pack(store, &resolved, &root, focus, budget);
    }
    Err(format!(
        "unknown target `{}`; use an entity id (ent:...), a requirement id (req:...), or a section reference (doc.md#/ref)",
        target
    ))
}

fn entity_pack(store: &Store, id: &str, focus: &Focus, budget: usize) -> Result<ContextPack, String> {
    let e = store.graph.entities.get(id).ok_or_else(|| format!("unknown entity {}", id))?;
    let mut b = Builder::new(budget);
    b.push(&format!("## Entity {} ({})", id, e.name));
    if let Some(d) = &e.definition {
        b.push(&format!("definition: {}", d));
    }
    if e.scope != "public" {
        b.push(&format!("scope: {}", e.scope));
    }
    if !e.aliases.is_empty() {
        b.push(&format!("aliases: {}", e.aliases.join(", ")));
    }

    if focus.mentions > 0 && !e.mentions.is_empty() {
        b.push("\n### Mentions");
        let items: Vec<String> = e
            .mentions
            .iter()
            .map(|m| format!("- {}#{} \"{}\"", m.doc, m.section, crate::llm::truncate(&m.quote, 160)))
            .collect();
        b.push_items(id, "mentions", 0, &items, "mentions");
        if focus.parents > 0 {
            for m in e.mentions.iter().take(1) {
                let chain = parent_chain(store, &m.doc, &m.section, focus.parents);
                if !chain.is_empty() {
                    b.push(&format!("  under: {}", chain.join(" → ")));
                }
            }
        }
    }

    let rids = reqs_of(store, id);
    if focus.requirements > 0 && !rids.is_empty() {
        b.push("\n### Requirements");
        let items: Vec<String> = rids.iter().map(|r| req_line(store, r, Some(id))).collect();
        b.push_items(id, "requirements", 0, &items, "requirements");
    }

    if focus.requirements > 1 {
        // Hop 2: entities tied through the requirements, one line each, then their statements.
        let mut related: Vec<String> = Vec::new();
        for rid in &rids {
            if let Some(r) = store.graph.requirements.get(rid) {
                for other in &r.entities {
                    let other = store.resolve_id(other).to_string();
                    if other != id && !related.contains(&other) {
                        related.push(other);
                    }
                }
            }
        }
        related.sort();
        if !related.is_empty() {
            b.push("\n### Related entities");
            let items: Vec<String> = related.iter().map(|r| entity_line(store, r)).collect();
            b.push_items(id, "related", 0, &items, "related entities");
        }
    }

    let diags: Vec<String> = store
        .graph
        .diagnostics
        .iter()
        .filter(|(_, d)| d.lifecycle == "open" && d.subjects.iter().any(|s| store.resolve_id(s) == id))
        .map(|(did, d)| format!("- {} [{}] {}: {}", did, d.severity, d.rule, d.message))
        .collect();
    if !diags.is_empty() {
        b.push("\n### Diagnostics");
        b.push_items(id, "diagnostics", 0, &diags, "diagnostics");
    }
    Ok(b.finish())
}

fn req_pack(store: &Store, id: &str, focus: &Focus, budget: usize) -> Result<ContextPack, String> {
    let r = store.graph.requirements.get(id).ok_or_else(|| format!("unknown requirement {}", id))?;
    let mut b = Builder::new(budget);
    b.push(&format!("## Requirement {}", id));
    b.push(&format!("ears: {}", r.ears));
    b.push(&format!("source: {}#{} \"{}\"", r.source.doc, r.source.section, crate::llm::truncate(&r.source.quote, 160)));
    b.push("\n### Entities");
    let items: Vec<String> = r.entities.iter().map(|e| entity_line(store, store.resolve_id(e))).collect();
    b.push_items(id, "entities", 0, &items, "entities");
    if focus.requirements > 1 {
        let mut sibs: Vec<String> = Vec::new();
        for e in &r.entities {
            for rid in reqs_of(store, store.resolve_id(e)) {
                if rid != id && !sibs.contains(&rid) {
                    sibs.push(rid);
                }
            }
        }
        sibs.sort();
        if !sibs.is_empty() {
            b.push("\n### Sibling requirements");
            let items: Vec<String> = sibs.iter().map(|s| req_line(store, s, None)).collect();
            b.push_items(id, "siblings", 0, &items, "sibling requirements");
        }
    }
    Ok(b.finish())
}

fn section_pack(store: &Store, doc: &str, sec: &str, focus: &Focus, budget: usize) -> Result<ContextPack, String> {
    let rec = store.docs.get(doc).ok_or_else(|| format!("unknown document {}", doc))?;
    let s = rec.sections.get(sec).ok_or_else(|| format!("unknown section {}#{}", doc, sec))?;
    let target = format!("{}#{}", doc, sec);
    let mut b = Builder::new(budget);
    b.push(&format!("## Section {} ({})", target, s.title));
    if let Some(c) = rec.coverage.get(sec) {
        b.push(&format!("coverage: {}", c.state));
    } else {
        b.push("coverage: unprocessed");
    }
    if focus.parents > 0 {
        let chain = parent_chain(store, doc, sec, focus.parents);
        if !chain.is_empty() {
            b.push(&format!("under: {}", chain.join(" → ")));
        }
        let children: Vec<String> = rec
            .sections
            .iter()
            .filter(|(_, c)| c.parent.as_deref() == Some(sec))
            .map(|(r, c)| format!("- {}#{} ({})", doc, r, c.title))
            .collect();
        if !children.is_empty() {
            b.push("children:");
            b.push_items(&target, "children", 0, &children, "child sections");
        }
    }
    b.push("\n### Body");
    if !b.push(&s.raw) {
        b.handles.push(Handle {
            handle: format!("h:{}|body|0", target),
            description: "full section body".to_string(),
            size: s.raw.len(),
        });
    }
    if focus.mentions > 0 {
        let ents: Vec<String> = store
            .graph
            .entities
            .iter()
            .filter(|(_, e)| e.mentions.iter().any(|m| m.doc == doc && m.section == sec))
            .map(|(id, _)| entity_line(store, id))
            .collect();
        if !ents.is_empty() {
            b.push("\n### Entities mentioned here");
            b.push_items(&target, "entities", 0, &ents, "entities");
        }
    }
    let reqs: Vec<String> = store
        .graph
        .requirements
        .iter()
        .filter(|(_, r)| r.source.doc == doc && r.source.section == sec)
        .map(|(rid, _)| req_line(store, rid, None))
        .collect();
    if !reqs.is_empty() {
        b.push("\n### Requirements sourced here");
        b.push_items(&target, "requirements", 0, &reqs, "requirements");
    }
    Ok(b.finish())
}

// Load the frontier behind a handle, under the same budget rules.
pub fn expand(store: &Store, handle: &str, budget: usize) -> Result<ContextPack, String> {
    let body = handle
        .strip_prefix("h:")
        .ok_or_else(|| format!("bad handle `{}`; handles start with h:", handle))?;
    let parts: Vec<&str> = body.split('|').collect();
    if parts.len() != 3 {
        return Err(format!("bad handle `{}`; expected h:<target>|<axis>|<start>", handle));
    }
    let (target, axis, start) = (parts[0], parts[1], parts[2].parse::<usize>().unwrap_or(0));
    let mut b = Builder::new(budget);
    b.push(&format!("## Expansion of {} ({})", target, axis));

    if axis == "body" {
        if let Some((doc, sec)) = split_section_ref(target) {
            let raw = store
                .docs
                .get(&doc)
                .and_then(|d| d.sections.get(&sec))
                .map(|s| s.raw.clone())
                .ok_or_else(|| format!("unknown section {}", target))?;
            let chunk: String = raw.chars().skip(start).take(budget.saturating_sub(200)).collect();
            let consumed = start + chunk.chars().count();
            b.push(&chunk);
            if consumed < raw.chars().count() {
                b.handles.push(Handle {
                    handle: format!("h:{}|body|{}", target, consumed),
                    description: format!("{} more chars", raw.chars().count() - consumed),
                    size: raw.len() - consumed,
                });
            }
            return Ok(b.finish());
        }
        return Err(format!("bad body handle `{}`", handle));
    }

    let items: Vec<String> = if target.starts_with("ent:") {
        let id = store.resolve_id(target).to_string();
        let e = store.graph.entities.get(&id).ok_or_else(|| format!("unknown entity {}", id))?;
        match axis {
            "mentions" => e
                .mentions
                .iter()
                .map(|m| format!("- {}#{} \"{}\"", m.doc, m.section, crate::llm::truncate(&m.quote, 160)))
                .collect(),
            "requirements" => reqs_of(store, &id).iter().map(|r| req_line(store, r, Some(&id))).collect(),
            "related" => {
                let mut related: Vec<String> = Vec::new();
                for rid in reqs_of(store, &id) {
                    if let Some(r) = store.graph.requirements.get(&rid) {
                        for other in &r.entities {
                            let other = store.resolve_id(other).to_string();
                            if other != id && !related.contains(&other) {
                                related.push(other);
                            }
                        }
                    }
                }
                related.sort();
                related.iter().map(|r| entity_line(store, r)).collect()
            }
            "diagnostics" => store
                .graph
                .diagnostics
                .iter()
                .filter(|(_, d)| d.lifecycle == "open" && d.subjects.iter().any(|s| store.resolve_id(s) == id))
                .map(|(did, d)| format!("- {} [{}] {}: {}", did, d.severity, d.rule, d.message))
                .collect(),
            _ => return Err(format!("unknown axis `{}` for entity handles", axis)),
        }
    } else if let Some((doc, sec)) = split_section_ref(target) {
        match axis {
            "children" => store
                .docs
                .get(&doc)
                .map(|rec| {
                    rec.sections
                        .iter()
                        .filter(|(_, c)| c.parent.as_deref() == Some(sec.as_str()))
                        .map(|(r, c)| format!("- {}#{} ({})", doc, r, c.title))
                        .collect()
                })
                .unwrap_or_default(),
            "entities" => store
                .graph
                .entities
                .iter()
                .filter(|(_, e)| e.mentions.iter().any(|m| m.doc == doc && m.section == sec))
                .map(|(id, _)| entity_line(store, id))
                .collect(),
            "requirements" => store
                .graph
                .requirements
                .iter()
                .filter(|(_, r)| r.source.doc == doc && r.source.section == sec)
                .map(|(rid, _)| req_line(store, rid, None))
                .collect(),
            _ => return Err(format!("unknown axis `{}` for section handles", axis)),
        }
    } else {
        return Err(format!("bad handle target `{}`", target));
    };
    b.push_items(target, axis, start, &items, axis);
    Ok(b.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use std::collections::BTreeMap;

    fn fixture() -> Store {
        let mut s = Store::default();
        let text = "# Shop\nintro\n\n## Cart\nThe Shopping Cart holds items.\n";
        s.docs.insert(
            "shop.md".into(),
            DocRecord { content_hash: hash_hex(text), sections: crate::md::parse_sections(text), coverage: BTreeMap::new() },
        );
        s.graph.entities.insert(
            "ent:shopping-cart".into(),
            Entity {
                name: "Shopping Cart".into(),
                definition: Some("holds items a customer intends to buy".into()),
                mentions: vec![SourceRef { doc: "shop.md".into(), section: "/shop/cart".into(), quote: "The Shopping Cart holds items.".into() }],
                ..Default::default()
            },
        );
        s.graph.entities.insert(
            "ent:customer".into(),
            Entity { name: "Customer".into(), definition: Some("a person who buys".into()), ..Default::default() },
        );
        for i in 0..6 {
            s.graph.requirements.insert(
                format!("req:shop-{}", i + 1),
                Requirement {
                    ears: format!("When event {} happens, the system shall update the Shopping Cart.", i + 1),
                    entities: vec!["ent:shopping-cart".into(), "ent:customer".into()],
                    edges: vec![],
                    source: SourceRef { doc: "shop.md".into(), section: "/shop/cart".into(), quote: "holds items".into() },
                    confidence: None, reasoning: None, created: None, updated: None,
                },
            );
        }
        s
    }

    #[test]
    fn entity_pack_within_budget_with_handles() {
        let s = fixture();
        let pack = assemble(&s, "ent:shopping-cart", &Focus::default(), 700).unwrap();
        assert!(pack.pack.len() <= 700 + 400); // handles section may add a little
        assert!(pack.pack.contains("Entity ent:shopping-cart"));
        assert!(!pack.handles.is_empty(), "small budget should cut requirements into a handle");
        let h = &pack.handles[0];
        let expansion = expand(&s, &h.handle, 4000).unwrap();
        assert!(expansion.pack.contains("req:shop-"));
    }

    #[test]
    fn big_budget_has_no_handles() {
        let s = fixture();
        let pack = assemble(&s, "ent:shopping-cart", &Focus::default(), 20_000).unwrap();
        assert!(pack.handles.is_empty());
        assert!(pack.pack.contains("req:shop-6"));
        assert!(pack.pack.contains("ent:customer"));
    }

    #[test]
    fn section_pack_renders_body_and_nodes() {
        let s = fixture();
        let pack = assemble(&s, "shop.md#/shop/cart", &Focus::default(), 20_000).unwrap();
        assert!(pack.pack.contains("The Shopping Cart holds items."));
        assert!(pack.pack.contains("Entities mentioned here"));
        assert!(pack.pack.contains("coverage: unprocessed"));
    }

    #[test]
    fn unknown_target_is_a_clear_error() {
        let s = fixture();
        let err = assemble(&s, "ent:nope", &Focus::default(), 1000).unwrap_err();
        assert!(err.contains("unknown entity"));
    }
}
