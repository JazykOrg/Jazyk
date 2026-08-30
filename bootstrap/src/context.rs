// The context engine. Two layers live here:
// - The loaded set: an explicit working set of graph nodes, bounded by a budget,
//   with a rendered status the model reads on every round. Owned by a session.
//   Mirrors docs/compiler/context.md.
// - The legacy bounded pack (`Focus`, `assemble`, `expand`): pure one-shot slices the
//   generation, binding, verification, LSP, and GUI surfaces still consume.
use crate::model::split_section_ref;
use crate::store::Store;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

// ---- the loaded set ----

// The axis names are a closed set; a handle without an axis names a stub's whole
// neighborhood. Mirrors docs/compiler/context.md#expansion-handles.
pub const AXES: [&str; 8] = [
    "parents",
    "mentions",
    "requirements",
    "related",
    "members",
    "children",
    "body",
    "edges",
];

// `h:<target>:<axis>[:<start>]`, parsed from the right (targets carry colons of
// their own). Returns (target, axis, start); axis is empty for a bare stub handle.
pub fn parse_handle(handle: &str) -> Result<(String, String, usize), String> {
    let body = handle
        .strip_prefix("h:")
        .ok_or_else(|| format!("bad handle `{}`; handles start with h:", handle))?;
    let mut parts: Vec<&str> = body.split(':').collect();
    if parts.is_empty() || parts[0].is_empty() {
        return Err(format!(
            "bad handle `{}`; expected h:<target>:<axis>[:<start>]",
            handle
        ));
    }
    let mut start = 0usize;
    if parts.len() >= 3 {
        if let Ok(n) = parts[parts.len() - 1].parse::<usize>() {
            if AXES.contains(&parts[parts.len() - 2]) {
                start = n;
                parts.pop();
            }
        }
    }
    let axis = if parts.len() >= 2 && AXES.contains(&parts[parts.len() - 1]) {
        parts.pop().unwrap().to_string()
    } else {
        String::new()
    };
    Ok((parts.join(":"), axis, start))
}

#[derive(Clone, Debug, Serialize)]
pub struct Handle {
    pub handle: String,
    pub description: String,
    pub size: usize,
}

// One item of the loaded set: a node loaded in full, a stub, or a section body.
#[derive(Clone, Debug)]
pub struct LoadedItem {
    pub target: String,
    // "full" | "stub" | "section body", with counts appended at render time.
    pub what: String,
    pub summary: String,
    pub chars: usize,
    pub handles: Vec<Handle>,
}

// The session's working set. Mirrors docs/compiler/context.md#the-loaded-set.
pub struct LoadedSet {
    pub items: Vec<LoadedItem>,
    pub budget: usize,
    // The character threshold past which load and expand refuse.
    pub high_water: usize,
    // The round each target was last named in a tool call's arguments.
    pub last_referenced: BTreeMap<String, u32>,
    pub round: u32,
    // The stored section bodies before this session's sync, read lazily from disk for
    // the dirty-section diff. None until first asked; Some(empty) when unavailable.
    prev_docs: Option<BTreeMap<String, BTreeMap<String, String>>>,
}

fn fmt_k(n: usize) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

fn first_line(s: &str) -> String {
    crate::llm::truncate(s.lines().next().unwrap_or(""), 160)
}

impl LoadedSet {
    pub fn new(budget: usize) -> LoadedSet {
        let budget = budget.max(400);
        LoadedSet {
            items: Vec::new(),
            budget,
            high_water: (budget as f64 * crate::limits::LOADED_HIGH_WATER) as usize,
            last_referenced: BTreeMap::new(),
            round: 0,
            prev_docs: None,
        }
    }

    pub fn used(&self) -> usize {
        self.items.iter().map(|i| i.chars).sum()
    }

    // Whether the set (plus the chars the caller already spent on skills) is past the
    // high-water mark, where load and expand refuse until something unloads.
    pub fn over_high_water(&self, extra_chars: usize) -> bool {
        self.used() + extra_chars > self.high_water
    }

    pub fn contains(&self, target: &str) -> bool {
        self.items.iter().any(|i| i.target == target)
    }

    pub fn next_round(&mut self) {
        self.round += 1;
    }

    pub fn note_reference(&mut self, target: &str) {
        let round = self.round;
        self.last_referenced.insert(target.to_string(), round);
    }

    pub fn open_handles(&self) -> Vec<String> {
        self.items
            .iter()
            .flat_map(|i| i.handles.iter().map(|h| h.handle.clone()))
            .collect()
    }

    // The unload candidates: least recently referenced first, skipping targets an
    // open goal names. Mirrors docs/compiler/context.md#policy.
    pub fn unload_candidates(&self, pinned: &BTreeSet<String>) -> Vec<String> {
        let mut c: Vec<(&LoadedItem, u32)> = self
            .items
            .iter()
            .filter(|i| !pinned.contains(&i.target))
            .map(|i| (i, self.last_referenced.get(&i.target).copied().unwrap_or(0)))
            .filter(|(_, last)| self.round.saturating_sub(*last) >= 6)
            .collect();
        c.sort_by_key(|(i, last)| (*last, i.target.clone()));
        c.into_iter().map(|(i, _)| i.target.clone()).collect()
    }

    // Drop an item; its handles close and its budget frees.
    pub fn unload(&mut self, target: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.target != target);
        self.items.len() != before
    }

    fn resolved<'a>(&self, store: &'a Store, target: &str) -> String {
        store.resolve_id(target).to_string()
    }

    // Load a target at a depth: the target in full, its edges, each neighbor as a
    // stub, neighbors' neighbors as counts. depth 2 loads the neighbors in full too.
    // Mirrors docs/compiler/context.md#policy.
    pub fn load(&mut self, store: &Store, target: &str, depth: u32) -> Result<String, String> {
        let id = self.resolved(store, target);
        let remaining = self.budget.saturating_sub(self.used()).max(400);
        let (text, item) = self.render_full(store, &id, remaining)?;
        self.items.retain(|i| i.target != item.target);
        self.items.push(item);
        self.note_reference(&id);
        let mut out = text;
        if depth >= 2 {
            let neighbors = self.neighbor_ids(store, &id);
            for n in neighbors {
                if self.contains(&n) || self.used() >= self.high_water {
                    continue;
                }
                let remaining = self.budget.saturating_sub(self.used()).max(400);
                if let Ok((t, item)) = self.render_full(store, &n, remaining) {
                    self.items.push(item);
                    out.push('\n');
                    out.push_str(&t);
                }
            }
        }
        Ok(out)
    }

    // A read's subject joins the set as a stub.
    pub fn load_stub(&mut self, store: &Store, target: &str) {
        let id = self.resolved(store, target);
        if self.contains(&id) {
            return;
        }
        let Some(line) = self.stub_line(store, &id) else {
            return;
        };
        let chars = line.len();
        self.items.push(LoadedItem {
            target: id.clone(),
            what: "stub".into(),
            summary: line,
            chars,
            handles: vec![Handle {
                handle: format!("h:{}", id),
                description: "the stub's whole neighborhood".into(),
                size: 400,
            }],
        });
    }

    // Load the frontier behind a handle. A closed or unknown handle errors naming the
    // open ones. Mirrors docs/compiler/context.md#expansion-handles.
    pub fn expand(&mut self, store: &Store, handle: &str) -> Result<String, String> {
        let (target, axis, start) = parse_handle(handle)?;
        let known = self
            .items
            .iter()
            .any(|i| i.target == target || i.handles.iter().any(|h| h.handle == handle));
        if !known {
            let open = self.open_handles();
            return Err(format!(
                "unknown or closed handle `{}`; open handles: {}",
                handle,
                if open.is_empty() {
                    "(none)".to_string()
                } else {
                    open.join(", ")
                }
            ));
        }
        if axis.is_empty() {
            // A stub's whole neighborhood: promote the stub to a full load.
            let text = self.load(store, &target, 1)?;
            return Ok(text);
        }
        self.note_reference(&target);
        let remaining = self.budget.saturating_sub(self.used()).max(400);
        if axis == "body" {
            let (doc, sec) = split_section_ref(&target)
                .ok_or_else(|| format!("bad body handle `{}`", handle))?;
            let raw = store
                .docs
                .get(&doc)
                .and_then(|d| d.sections.get(&sec))
                .map(|s| s.raw.clone())
                .ok_or_else(|| format!("unknown section {}", target))?;
            let chunk: String = raw
                .chars()
                .skip(start)
                .take(remaining.saturating_sub(200))
                .collect();
            let consumed = start + chunk.chars().count();
            let mut out = format!("## Expansion of {} (body)\n{}", target, chunk);
            let mut handles = Vec::new();
            if consumed < raw.chars().count() {
                handles.push(Handle {
                    handle: format!("h:{}:body:{}", target, consumed),
                    description: format!("{} more chars", raw.chars().count() - consumed),
                    size: raw.len() - consumed,
                });
                out.push_str(&format!("\n[h:{}:body:{}]", target, consumed));
            }
            self.absorb_expansion(&target, chunk.len(), &axis, handles);
            return Ok(out);
        }
        let items = self.axis_items(store, &target, &axis)?;
        let mut out = format!("## Expansion of {} ({})\n", target, axis);
        let mut used = out.len();
        let mut cut: Option<(usize, usize)> = None;
        for (i, line) in items.iter().enumerate().skip(start) {
            if used + line.len() + 1 > remaining {
                let size: usize = items[i..].iter().map(|s| s.len() + 1).sum();
                cut = Some((i, size));
                break;
            }
            out.push_str(line);
            out.push('\n');
            used += line.len() + 1;
        }
        let mut handles = Vec::new();
        if let Some((i, size)) = cut {
            let h = format!("h:{}:{}:{}", target, axis, i);
            out.push_str(&format!("[{} more: {}]", items.len() - i, h));
            handles.push(Handle {
                handle: h,
                description: format!("{} more {}", items.len() - i, axis),
                size,
            });
        }
        self.absorb_expansion(&target, out.len(), &axis, handles);
        Ok(out)
    }

    // Record what an expansion added: the chars count against the item, the axis
    // handle is replaced by the continuation (or closed).
    fn absorb_expansion(&mut self, target: &str, chars: usize, axis: &str, handles: Vec<Handle>) {
        if let Some(item) = self.items.iter_mut().find(|i| i.target == target) {
            item.chars += chars;
            item.handles.retain(|h| {
                parse_handle(&h.handle)
                    .map(|(_, a, _)| a != axis)
                    .unwrap_or(true)
            });
            item.handles.extend(handles);
        }
    }

    fn neighbor_ids(&self, store: &Store, id: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if let Some(e) = store.graph.entities.get(id) {
            if let Some(p) = &e.parent {
                out.push(p.clone());
            }
            for rid in store.requirements_referencing(id) {
                if let Some(r) = store.graph.requirements.get(&rid) {
                    for other in &r.entities {
                        let other = store.resolve_id(other).to_string();
                        if other != id && !out.contains(&other) {
                            out.push(other);
                        }
                    }
                }
            }
        }
        if let Some(r) = store.graph.requirements.get(id) {
            for e in &r.entities {
                let e = store.resolve_id(e).to_string();
                if !out.contains(&e) {
                    out.push(e);
                }
            }
        }
        out
    }

    // The list an axis names, one rendered line per item.
    fn axis_items(&self, store: &Store, target: &str, axis: &str) -> Result<Vec<String>, String> {
        if let Some((doc, sec)) = split_section_ref(target) {
            return Ok(match axis {
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
                "mentions" => store
                    .graph
                    .entities
                    .iter()
                    .filter(|(_, e)| e.mentions.iter().any(|m| m.doc == doc && m.section == sec))
                    .filter_map(|(id, _)| self.stub_line(store, id))
                    .collect(),
                "requirements" => store
                    .graph
                    .requirements
                    .iter()
                    .filter(|(_, r)| r.anchored_at(&doc, &sec))
                    .map(|(rid, r)| format!("- {}: {}", rid, r.statement))
                    .collect(),
                "parents" => {
                    let mut chain = Vec::new();
                    let mut cur = store
                        .docs
                        .get(&doc)
                        .and_then(|rec| rec.sections.get(&sec))
                        .and_then(|s| s.parent.clone());
                    while let Some(p) = cur {
                        let title = store
                            .docs
                            .get(&doc)
                            .and_then(|rec| rec.sections.get(&p))
                            .map(|s| s.title.clone())
                            .unwrap_or_default();
                        chain.push(format!("- {}#{} ({})", doc, p, title));
                        cur = store
                            .docs
                            .get(&doc)
                            .and_then(|rec| rec.sections.get(&p))
                            .and_then(|s| s.parent.clone());
                    }
                    chain
                }
                _ => return Err(format!("axis `{}` does not apply to a section", axis)),
            });
        }
        if let Some(e) = store.graph.entities.get(target) {
            return Ok(match axis {
                "parents" => {
                    let mut chain = Vec::new();
                    let mut cur = e.parent.clone();
                    while let Some(p) = cur {
                        chain.push(
                            self.stub_line(store, &p)
                                .unwrap_or_else(|| format!("- {}", p)),
                        );
                        cur = store.graph.entities.get(&p).and_then(|x| x.parent.clone());
                    }
                    chain
                }
                "children" => store
                    .graph
                    .entities
                    .iter()
                    .filter(|(_, c)| c.parent.as_deref() == Some(target))
                    .filter_map(|(id, _)| self.stub_line(store, id))
                    .collect(),
                "mentions" => e
                    .mentions
                    .iter()
                    .map(|m| {
                        format!(
                            "- {}#{} \"{}\"",
                            m.doc,
                            m.section,
                            crate::llm::truncate(&m.quote, 160)
                        )
                    })
                    .collect(),
                "requirements" => {
                    let mut v = store.requirements_referencing(target);
                    v.sort();
                    v.iter()
                        .filter_map(|rid| {
                            store
                                .graph
                                .requirements
                                .get(rid)
                                .map(|r| format!("- {}: {}", rid, r.statement))
                        })
                        .collect()
                }
                "related" => related_lines(store, target),
                "members" => store
                    .graph
                    .views
                    .iter()
                    .filter(|(_, v)| v.members.iter().any(|m| store.resolve_id(m) == target))
                    .map(|(vid, v)| format!("- {} ({})", vid, v.title))
                    .collect(),
                _ => return Err(format!("axis `{}` does not apply to an entity", axis)),
            });
        }
        if let Some(r) = store.graph.requirements.get(target) {
            return Ok(match axis {
                "requirements" | "related" => r
                    .entities
                    .iter()
                    .filter_map(|e| self.stub_line(store, store.resolve_id(e)))
                    .collect(),
                "members" => store
                    .graph
                    .views
                    .iter()
                    .filter(|(_, v)| v.members.iter().any(|m| m == target))
                    .map(|(vid, v)| format!("- {} ({})", vid, v.title))
                    .collect(),
                _ => return Err(format!("axis `{}` does not apply to a requirement", axis)),
            });
        }
        if let Some(v) = store.graph.views.get(target) {
            return Ok(match axis {
                "members" => v
                    .members
                    .iter()
                    .filter_map(|m| self.stub_line(store, store.resolve_id(m)))
                    .collect(),
                "edges" => view_edge_lines(store, v),
                _ => return Err(format!("axis `{}` does not apply to a view", axis)),
            });
        }
        Err(format!("unknown target `{}`", target))
    }

    // One line per policy: name, one definition line, the stereotype, its own edge
    // count. Mirrors docs/compiler/context.md#policy.
    pub fn stub_line(&self, store: &Store, id: &str) -> Option<String> {
        if let Some(e) = store.graph.entities.get(id) {
            let edges = store
                .graph
                .relationships
                .values()
                .filter(|r| r.members.contains(&id.to_string()))
                .count();
            let st = e
                .stereotype
                .as_ref()
                .map(|s| format!(" «{}»", s))
                .unwrap_or_default();
            return Some(format!(
                "- {} ({}){} {} ({} edges) [h:{}]",
                id,
                e.name,
                st,
                first_line(e.definition.as_deref().unwrap_or("(no definition yet)")),
                edges,
                id
            ));
        }
        if let Some(r) = store.graph.requirements.get(id) {
            return Some(format!(
                "- {}: {} ({})",
                id,
                crate::llm::truncate(&r.statement, 160),
                r.entities.join(", ")
            ));
        }
        if let Some(v) = store.graph.views.get(id) {
            return Some(format!(
                "- {} ({}, {}, {} members)",
                id,
                v.title,
                v.kind,
                v.members.len()
            ));
        }
        if let Some((doc, sec)) = split_section_ref(id) {
            let rec = store.docs.get(&doc)?;
            let s = rec.sections.get(&sec)?;
            let cov = rec
                .coverage
                .get(&sec)
                .map(|c| c.state.clone())
                .unwrap_or_else(|| "unprocessed".into());
            return Some(format!("- {} ({}) [coverage: {}]", id, s.title, cov));
        }
        if let Some(d) = store.graph.diagnostics.get(id) {
            return Some(format!(
                "- {} [{}] {}: {}",
                id,
                d.severity,
                d.rule,
                crate::llm::truncate(&d.message, 120)
            ));
        }
        None
    }

    // Render one target in full under a byte budget, with the handles for what was cut.
    fn render_full(
        &mut self,
        store: &Store,
        id: &str,
        budget: usize,
    ) -> Result<(String, LoadedItem), String> {
        let mut b = Builder::new(budget);
        let (what, summary) = if let Some(e) = store.graph.entities.get(id) {
            self.render_entity(store, id, e, &mut b);
            let reqs = store.requirements_referencing(id).len();
            let parent = e
                .parent
                .as_ref()
                .map(|p| format!(", parent {}", p))
                .unwrap_or_default();
            (
                "full".to_string(),
                format!("full: {} requirements{}", reqs, parent),
            )
        } else if let Some(r) = store.graph.requirements.get(id) {
            self.render_requirement(store, id, r, &mut b);
            let src = match r.source.as_ref() {
                Some(s) => format!("source {}#{}", s.doc, s.section),
                None => "derived".to_string(),
            };
            ("full".to_string(), format!("full: statement, {}", src))
        } else if let Some(v) = store.graph.views.get(id) {
            let shown = self.render_view(store, id, v, &mut b);
            (
                "full".to_string(),
                format!(
                    "{} of {} members shown ({})",
                    shown,
                    v.members.len(),
                    v.kind
                ),
            )
        } else if let Some(d) = store.graph.diagnostics.get(id) {
            b.push(&format!(
                "## Diagnostic {} ({}, {})",
                id, d.rule, d.severity
            ));
            b.push(&format!("message: {}", d.message));
            for s in &d.subjects {
                if let Some(line) = self.stub_line(store, s) {
                    b.push(&line);
                }
            }
            ("full".to_string(), format!("full: {}", d.rule))
        } else if let Some((doc, sec)) = split_section_ref(id) {
            self.render_section(store, &doc, &sec, &mut b)?;
            ("section body".to_string(), "section body".to_string())
        } else if store.docs.contains_key(id) {
            let root = store
                .docs
                .get(id)
                .and_then(|d| {
                    d.sections
                        .iter()
                        .find(|(_, s)| s.kind == "root")
                        .map(|(r, _)| r.clone())
                })
                .ok_or_else(|| format!("document {} has no root section", id))?;
            self.render_section(store, id, &root, &mut b)?;
            ("section body".to_string(), "document root".to_string())
        } else {
            return Err(format!(
                "unknown target `{}`; use a node id (ent:..., req:..., view:..., diag:...) or a section reference (doc.md#/ref)",
                id
            ));
        };
        let pack = b.finish();
        let chars = pack.pack.len();
        Ok((
            pack.pack,
            LoadedItem {
                target: id.to_string(),
                what,
                summary,
                chars,
                handles: pack.handles,
            },
        ))
    }

    fn render_entity(&self, store: &Store, id: &str, e: &crate::model::Entity, b: &mut Builder) {
        let st = e
            .stereotype
            .as_ref()
            .map(|s| format!("  «{}»", s))
            .unwrap_or_default();
        b.push(&format!("## {} ({}){}  full", id, e.name, st));
        let mut head = Vec::new();
        if e.scope != "public" {
            head.push(format!("scope: {}", e.scope));
        }
        if let Some(p) = &e.parent {
            let pname = store
                .graph
                .entities
                .get(p)
                .map(|x| x.name.clone())
                .unwrap_or_default();
            head.push(format!("parent: {} ({})", p, pname));
        }
        if !head.is_empty() {
            b.push(&head.join("   "));
        }
        if let Some(d) = &e.definition {
            b.push(&format!("definition: {}", d));
        }
        if !e.aliases.is_empty() {
            b.push(&format!("aliases: {}", e.aliases.join(", ")));
        }
        if !e.attributes.is_empty() {
            b.push(&format!(
                "attributes: {}",
                e.attributes
                    .iter()
                    .map(|a| {
                        let mut s = a.name.clone();
                        if let Some(t) = &a.r#type {
                            s.push_str(&format!(": {}", t));
                        }
                        if let Some(v) = &a.value {
                            s.push_str(&format!(" = {}", v));
                        }
                        s
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !e.limits.is_empty() {
            b.push(&format!(
                "limits: {}",
                e.limits
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.value))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let mentions: Vec<String> = e
            .mentions
            .iter()
            .map(|m| {
                format!(
                    "- {}#{} \"{}\"",
                    m.doc,
                    m.section,
                    crate::llm::truncate(&m.quote, 160)
                )
            })
            .collect();
        if !mentions.is_empty() {
            b.push("mentions:");
            b.push_items(id, "mentions", &mentions);
        }
        let mut rids = store.requirements_referencing(id);
        rids.sort();
        if !rids.is_empty() {
            b.push(&format!("requirements ({}):", rids.len()));
            let lines: Vec<String> = rids
                .iter()
                .filter_map(|rid| {
                    store.graph.requirements.get(rid).map(|r| {
                        let mut s = format!("- {}: {}", rid, r.statement);
                        if let Some(t) = &r.transition {
                            s.push_str(&format!(
                                "\n  transition: {} → {}{}",
                                t.from,
                                t.to,
                                t.trigger
                                    .as_ref()
                                    .map(|x| format!(" ({})", x))
                                    .unwrap_or_default()
                            ));
                        }
                        if !r.facets.is_empty() {
                            s.push_str(&format!(
                                "   facets: {}",
                                r.facets
                                    .iter()
                                    .map(|f| f.facet.clone())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                        }
                        s
                    })
                })
                .collect();
            b.push_items(id, "requirements", &lines);
        }
        let related = related_lines(store, id);
        if !related.is_empty() {
            b.push("related:");
            b.push_items(id, "related", &related);
        }
        let views: Vec<String> = store
            .graph
            .views
            .iter()
            .filter(|(_, v)| v.members.iter().any(|m| store.resolve_id(m) == id))
            .map(|(vid, _)| vid.clone())
            .collect();
        if !views.is_empty() {
            b.push(&format!("views: {}", views.join(", ")));
        }
        let slug = crate::derive::entity_slug(id);
        if let Some(sm) = store.graph.state_machines.get(&format!("sm:{}", slug)) {
            b.push(&format!(
                "state machine sm:{}: {} states, {} transitions",
                slug,
                sm.states.len(),
                sm.transitions.len()
            ));
        }
        // Neighbors as stubs; their own neighborhoods sit behind their handles.
        let neighbors = self.neighbor_ids(store, id);
        if !neighbors.is_empty() {
            b.push("\nneighbors:");
            let lines: Vec<String> = neighbors
                .iter()
                .filter(|n| !self.contains(n))
                .filter_map(|n| self.stub_line(store, n))
                .collect();
            b.push_items(id, "children", &lines);
        }
    }

    fn render_requirement(
        &self,
        store: &Store,
        id: &str,
        r: &crate::model::Requirement,
        b: &mut Builder,
    ) {
        b.push(&format!("## {}  full", id));
        b.push(&format!("statement: {}", r.statement));
        match r.source.as_ref() {
            Some(src) => {
                b.push(&format!(
                    "source: {}#{} \"{}\"",
                    src.doc,
                    src.section,
                    crate::llm::truncate(&src.quote, 200)
                ));
            }
            None => {
                b.push(&format!(
                    "provenance: {}",
                    crate::session::provenance_line(r)
                ));
            }
        }
        if let Some(t) = &r.transition {
            b.push(&format!(
                "transition: {} {} → {}{}{}",
                t.subject,
                t.from,
                t.to,
                t.trigger
                    .as_ref()
                    .map(|x| format!(" ({})", x))
                    .unwrap_or_default(),
                t.guard
                    .as_ref()
                    .map(|g| format!(" [{}]", g))
                    .unwrap_or_default()
            ));
        }
        if !r.facets.is_empty() {
            b.push(&format!(
                "facets: {}",
                r.facets
                    .iter()
                    .map(|f| {
                        let mut s = f.facet.clone();
                        if let Some(m) = &f.measure {
                            s.push_str(&format!(" ({})", m));
                        }
                        s
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !r.edges.is_empty() {
            b.push(&format!(
                "edges: {}",
                r.edges
                    .iter()
                    .map(|e| {
                        format!(
                            "{} → {}{}{}",
                            e.a,
                            e.b,
                            e.rel_type
                                .as_ref()
                                .map(|t| format!(" ({})", t))
                                .unwrap_or_default(),
                            e.cardinality
                                .as_ref()
                                .map(|c| format!(" {}", c))
                                .unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        let lines: Vec<String> = r
            .entities
            .iter()
            .filter_map(|e| self.stub_line(store, store.resolve_id(e)))
            .collect();
        if !lines.is_empty() {
            b.push("entities:");
            b.push_items(id, "requirements", &lines);
        }
        let views: Vec<String> = store
            .graph
            .views
            .iter()
            .filter(|(_, v)| v.members.iter().any(|m| m == id))
            .map(|(vid, _)| vid.clone())
            .collect();
        if !views.is_empty() {
            b.push(&format!("views: {}", views.join(", ")));
        }
    }

    // Renders the view; returns how many members were shown.
    fn render_view(
        &self,
        store: &Store,
        id: &str,
        v: &crate::model::View,
        b: &mut Builder,
    ) -> usize {
        b.push(&format!("## {} ({}, {})  full", id, v.title, v.kind));
        let mut shown = 0usize;
        let lines: Vec<String> = v
            .members
            .iter()
            .filter_map(|m| self.stub_line(store, store.resolve_id(m)))
            .collect();
        if !lines.is_empty() {
            b.push(&format!("members ({}):", v.members.len()));
            shown = b.push_items_counted(id, "members", &lines, "members unloaded");
        }
        if !v.excluded.is_empty() {
            b.push(&format!(
                "excluded: {}",
                v.excluded
                    .iter()
                    .map(|x| format!("{} ({})", x.id, x.note))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if !v.collapse.is_empty() {
            b.push(&format!("collapse: {}", v.collapse.join(", ")));
        }
        if let Some(q) = &v.query {
            b.push(&format!(
                "query: scope {:?}, parent {:?}, stereotype {:?}",
                q.scope, q.parent, q.stereotype
            ));
        }
        let edges = view_edge_lines(store, v);
        if !edges.is_empty() {
            b.push(&format!("relationships among members ({}):", edges.len()));
            b.push_items(id, "edges", &edges);
        }
        shown
    }

    fn render_section(
        &mut self,
        store: &Store,
        doc: &str,
        sec: &str,
        b: &mut Builder,
    ) -> Result<(), String> {
        let rec = store
            .docs
            .get(doc)
            .ok_or_else(|| format!("unknown document {}", doc))?;
        let s = rec
            .sections
            .get(sec)
            .ok_or_else(|| format!("unknown section {}#{}", doc, sec))?;
        let target = format!("{}#{}", doc, sec);
        b.push(&format!("## Section {} ({})", target, s.title));
        let cov = rec
            .coverage
            .get(sec)
            .map(|c| c.state.clone())
            .unwrap_or_else(|| "unprocessed".to_string());
        b.push(&format!("coverage: {}", cov));
        let children: Vec<String> = rec
            .sections
            .iter()
            .filter(|(_, c)| c.parent.as_deref() == Some(sec))
            .map(|(r, c)| format!("- {}#{} ({})", doc, r, c.title))
            .collect();
        if !children.is_empty() {
            b.push("children:");
            b.push_items(&target, "children", &children);
        }
        // A dirty section renders with the diff against its last reconciled body
        // marked. Mirrors docs/compiler/context.md#policy.
        let dirty = store
            .status
            .changes
            .iter()
            .any(|c| c.kind == "section-dirty" && c.subject == target);
        let body = if dirty {
            match self.previous_body(store, doc, sec) {
                Some(prev) if prev != s.raw => mark_diff(&prev, &s.raw),
                Some(_) => s.raw.clone(),
                None => format!("(changed; previous body unavailable)\n{}", s.raw),
            }
        } else {
            s.raw.clone()
        };
        if !b.push(&body) {
            b.handles.push(Handle {
                handle: format!("h:{}:body:0", target),
                description: "full section body".to_string(),
                size: body.len(),
            });
        }
        let reqs: Vec<String> = store
            .graph
            .requirements
            .iter()
            .filter(|(_, r)| r.anchored_at(doc, sec))
            .map(|(rid, r)| format!("- {}: {}", rid, r.statement))
            .collect();
        if !reqs.is_empty() {
            b.push("requirements sourced here:");
            b.push_items(&target, "requirements", &reqs);
        }
        let ents: Vec<String> = store
            .graph
            .entities
            .iter()
            .filter(|(_, e)| e.mentions.iter().any(|m| m.doc == doc && m.section == sec))
            .filter_map(|(id, _)| self.stub_line(store, id))
            .collect();
        if !ents.is_empty() {
            b.push("entities mentioned here:");
            b.push_items(&target, "mentions", &ents);
        }
        Ok(())
    }

    // The stored body before this session's sync, read once from disk.
    fn previous_body(&mut self, store: &Store, doc: &str, sec: &str) -> Option<String> {
        if self.prev_docs.is_none() {
            let mut map: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
            if !store.out.as_os_str().is_empty() {
                let disk = Store::load(&store.out);
                for (d, rec) in disk.docs {
                    map.insert(
                        d,
                        rec.sections.into_iter().map(|(r, s)| (r, s.raw)).collect(),
                    );
                }
            }
            self.prev_docs = Some(map);
        }
        self.prev_docs
            .as_ref()
            .and_then(|m| m.get(doc))
            .and_then(|d| d.get(sec))
            .cloned()
    }

    // The status block. Mirrors docs/compiler/context.md#rendering.
    pub fn render_status(
        &self,
        skill_line: &str,
        skill_chars: usize,
        pinned: &BTreeSet<String>,
    ) -> String {
        let mut s = format!(
            "## Loaded ({}/{} chars)\n",
            fmt_k(self.used() + skill_chars),
            fmt_k(self.budget)
        );
        for i in &self.items {
            let handles: String = i
                .handles
                .iter()
                .map(|h| format!("  [{}: {}]", h.description, h.handle))
                .collect();
            s.push_str(&format!("- {}   {}{}\n", i.target, i.summary, handles));
        }
        if !skill_line.is_empty() {
            s.push_str(skill_line);
            s.push('\n');
        }
        let candidates = self.unload_candidates(pinned);
        if !candidates.is_empty() {
            let c = &candidates[0];
            let last = self.last_referenced.get(c).copied().unwrap_or(0);
            s.push_str(&format!(
                "Consider unloading: {} (not referenced in {} rounds, no open goal touches it)\n",
                c,
                self.round.saturating_sub(last)
            ));
        }
        s
    }

    // The condensed form: the header and the item lines alone.
    pub fn render_condensed(&self, skill_chars: usize) -> String {
        let mut s = format!(
            "## Loaded ({}/{} chars)\n",
            fmt_k(self.used() + skill_chars),
            fmt_k(self.budget)
        );
        for i in &self.items {
            s.push_str(&format!("- {}   {}\n", i.target, i.summary));
        }
        s
    }
}

// The relationship lines of an entity: every contribution group, with the entity on
// the other end. Mirrors docs/compiler/context.md#axes.
fn related_lines(store: &Store, id: &str) -> Vec<String> {
    let mut out = Vec::new();
    for rel in store.graph.relationships.values() {
        if !rel.members.contains(&id.to_string()) {
            continue;
        }
        let other = rel
            .members
            .iter()
            .find(|m| m.as_str() != id)
            .cloned()
            .unwrap_or_else(|| id.to_string());
        let name = store
            .graph
            .entities
            .get(&other)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        for c in &rel.contributions {
            out.push(format!(
                "- {} ({}): {}{} ({} requirement(s))",
                other,
                name,
                c.r#type,
                c.cardinality
                    .as_ref()
                    .map(|x| format!(" {}", x))
                    .unwrap_or_default(),
                c.requirements.len()
            ));
        }
    }
    out.sort();
    out.dedup();
    out
}

// The arrows among a view's members, one line per contribution group.
fn view_edge_lines(store: &Store, v: &crate::model::View) -> Vec<String> {
    let members: BTreeSet<&str> = v.members.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for rel in store.graph.relationships.values() {
        if !rel.members.iter().all(|m| members.contains(m.as_str())) {
            continue;
        }
        for c in &rel.contributions {
            out.push(format!(
                "- {} → {} ({}, {})",
                c.a,
                c.b,
                c.r#type,
                c.requirements.len()
            ));
        }
    }
    out.sort();
    out.dedup();
    out
}

// A crude line diff for a dirty section: lines the previous body lacked are marked
// `+`, lines it lost are listed under a `removed:` trailer.
fn mark_diff(prev: &str, cur: &str) -> String {
    let prev_lines: BTreeSet<&str> = prev.lines().collect();
    let cur_lines: BTreeSet<&str> = cur.lines().collect();
    let mut out = String::new();
    for l in cur.lines() {
        if prev_lines.contains(l) {
            out.push_str(l);
        } else {
            out.push_str("+ ");
            out.push_str(l);
        }
        out.push('\n');
    }
    let removed: Vec<&str> = prev
        .lines()
        .filter(|l| !l.trim().is_empty() && !cur_lines.contains(*l))
        .collect();
    if !removed.is_empty() {
        out.push_str("removed since last reconcile:\n");
        for l in removed {
            out.push_str("- ");
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

// `jazyk context <target>`: exactly what `load` renders, then the status block, with
// `--expand` following named handles first. Mirrors docs/frontends/cli.md#jazyk-context.
pub fn cli_context(
    store: &Store,
    target: &str,
    depth: u32,
    expands: &[String],
) -> Result<String, String> {
    let mut set = LoadedSet::new(crate::limits::CONTEXT_BUDGET);
    let mut out = if target.starts_with("h:") {
        // A handle needs its item loaded first.
        let (t, _, _) = parse_handle(target)?;
        set.load(store, &t, depth)?;
        set.expand(store, target)?
    } else {
        set.load(store, target, depth)?
    };
    for h in expands {
        out.push('\n');
        out.push_str(&set.expand(store, h)?);
    }
    out.push('\n');
    out.push_str(&set.render_status("", 0, &BTreeSet::new()));
    Ok(out)
}

// ---- the legacy bounded pack (Focus/assemble/expand) ----
// Consumed by gen.rs, bind.rs, verify.rs, lsp.rs, and the GUI until their
// workstreams move to the loaded set.

#[derive(Clone, Copy)]
pub struct Focus {
    pub parents: u32,
    pub mentions: u32,
    pub requirements: u32,
}

impl Default for Focus {
    fn default() -> Self {
        Focus {
            parents: 2,
            mentions: 1,
            requirements: 2,
        }
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
        Builder {
            budget: budget.max(400),
            text: String::new(),
            handles: Vec::new(),
        }
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
    fn push_items(&mut self, target: &str, axis: &str, items: &[String]) {
        self.push_items_counted(target, axis, items, axis);
    }
    // Same, reporting how many items landed and naming the cut in `what`'s words.
    fn push_items_counted(
        &mut self,
        target: &str,
        axis: &str,
        items: &[String],
        what: &str,
    ) -> usize {
        for (i, item) in items.iter().enumerate() {
            if !self.push(item) {
                let remaining = items.len() - i;
                let size: usize = items[i..].iter().map(|s| s.len() + 1).sum();
                let handle = if i == 0 {
                    format!("h:{}:{}", target, axis)
                } else {
                    format!("h:{}:{}:{}", target, axis, i)
                };
                self.text
                    .push_str(&format!("({} {} [{}])\n", remaining, what, handle));
                self.handles.push(Handle {
                    handle,
                    description: format!("{} more {}", remaining, what),
                    size,
                });
                return i;
            }
        }
        items.len()
    }
    fn finish(self) -> ContextPack {
        ContextPack {
            pack: self.text,
            handles: self.handles,
        }
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
                format!("- {}: {}", rid, r.statement)
            } else {
                format!("- {}: {} (ties: {})", rid, r.statement, ties.join(", "))
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
    let Some(rec) = store.docs.get(doc) else {
        return chain;
    };
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
pub fn assemble(
    store: &Store,
    target: &str,
    focus: &Focus,
    budget: usize,
) -> Result<ContextPack, String> {
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
            .and_then(|d| {
                d.sections
                    .iter()
                    .find(|(_, s)| s.kind == "root")
                    .map(|(r, _)| r.clone())
            })
            .ok_or_else(|| format!("document {} has no root section", resolved))?;
        return section_pack(store, &resolved, &root, focus, budget);
    }
    Err(format!(
        "unknown target `{}`; use an entity id (ent:...), a requirement id (req:...), or a section reference (doc.md#/ref)",
        target
    ))
}

fn entity_pack(
    store: &Store,
    id: &str,
    focus: &Focus,
    budget: usize,
) -> Result<ContextPack, String> {
    let e = store
        .graph
        .entities
        .get(id)
        .ok_or_else(|| format!("unknown entity {}", id))?;
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
            .map(|m| {
                format!(
                    "- {}#{} \"{}\"",
                    m.doc,
                    m.section,
                    crate::llm::truncate(&m.quote, 160)
                )
            })
            .collect();
        b.push_items(id, "mentions", &items);
        if focus.parents > 0 {
            for m in e.mentions.iter().take(1) {
                let chain = parent_chain(store, &m.doc, &m.section, focus.parents);
                if !chain.is_empty() {
                    b.push(&format!(
                        "  (the first mention's section sits under: {})",
                        chain.join(" → ")
                    ));
                }
            }
        }
    }

    let rids = reqs_of(store, id);
    if focus.requirements > 0 && !rids.is_empty() {
        b.push("\n### Requirements");
        let items: Vec<String> = rids.iter().map(|r| req_line(store, r, Some(id))).collect();
        b.push_items(id, "requirements", &items);
    }

    if focus.requirements > 1 {
        // Hop 2: entities tied through the requirements, one line each.
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
            b.push_items(id, "related", &items);
        }
    }

    let diags: Vec<String> = store
        .graph
        .diagnostics
        .iter()
        .filter(|(_, d)| {
            d.lifecycle == "open" && d.subjects.iter().any(|s| store.resolve_id(s) == id)
        })
        .map(|(did, d)| format!("- {} [{}] {}: {}", did, d.severity, d.rule, d.message))
        .collect();
    if !diags.is_empty() {
        b.push("\n### Diagnostics");
        b.push_items(id, "related", &diags);
    }
    Ok(b.finish())
}

fn req_pack(store: &Store, id: &str, focus: &Focus, budget: usize) -> Result<ContextPack, String> {
    let r = store
        .graph
        .requirements
        .get(id)
        .ok_or_else(|| format!("unknown requirement {}", id))?;
    let mut b = Builder::new(budget);
    b.push(&format!("## Requirement {}", id));
    b.push(&format!("statement: {}", r.statement));
    match r.source.as_ref() {
        Some(src) => {
            b.push(&format!(
                "source: {}#{} \"{}\"",
                src.doc,
                src.section,
                crate::llm::truncate(&src.quote, 160)
            ));
        }
        None => {
            b.push(&format!(
                "provenance: {}",
                crate::session::provenance_line(r)
            ));
        }
    }
    b.push("\n### Entities");
    let items: Vec<String> = r
        .entities
        .iter()
        .map(|e| entity_line(store, store.resolve_id(e)))
        .collect();
    b.push_items(id, "requirements", &items);
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
            b.push_items(id, "related", &items);
        }
    }
    Ok(b.finish())
}

fn section_pack(
    store: &Store,
    doc: &str,
    sec: &str,
    focus: &Focus,
    budget: usize,
) -> Result<ContextPack, String> {
    let rec = store
        .docs
        .get(doc)
        .ok_or_else(|| format!("unknown document {}", doc))?;
    let s = rec
        .sections
        .get(sec)
        .ok_or_else(|| format!("unknown section {}#{}", doc, sec))?;
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
            b.push_items(&target, "children", &children);
        }
    }
    b.push("\n### Body");
    if !b.push(&s.raw) {
        b.handles.push(Handle {
            handle: format!("h:{}:body:0", target),
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
            b.push_items(&target, "mentions", &ents);
        }
    }
    let reqs: Vec<String> = store
        .graph
        .requirements
        .iter()
        .filter(|(_, r)| r.anchored_at(doc, sec))
        .map(|(rid, _)| req_line(store, rid, None))
        .collect();
    if !reqs.is_empty() {
        b.push("\n### Requirements sourced here");
        b.push_items(&target, "requirements", &reqs);
    }
    Ok(b.finish())
}

// Load the frontier behind a handle, one-shot over the snapshot (the legacy surface;
// a session's handles go through LoadedSet::expand).
pub fn expand(store: &Store, handle: &str, budget: usize) -> Result<ContextPack, String> {
    let (target, axis, start) = parse_handle(handle)?;
    if axis.is_empty() {
        return Err(format!(
            "handle `{}` names no axis; one of: {}",
            handle,
            AXES.join(", ")
        ));
    }
    let mut b = Builder::new(budget);
    b.push(&format!("## Expansion of {} ({})", target, axis));

    if axis == "body" {
        if let Some((doc, sec)) = split_section_ref(&target) {
            let raw = store
                .docs
                .get(&doc)
                .and_then(|d| d.sections.get(&sec))
                .map(|s| s.raw.clone())
                .ok_or_else(|| format!("unknown section {}", target))?;
            let chunk: String = raw
                .chars()
                .skip(start)
                .take(budget.saturating_sub(200))
                .collect();
            let consumed = start + chunk.chars().count();
            b.push(&chunk);
            if consumed < raw.chars().count() {
                b.handles.push(Handle {
                    handle: format!("h:{}:body:{}", target, consumed),
                    description: format!("{} more chars", raw.chars().count() - consumed),
                    size: raw.len() - consumed,
                });
            }
            return Ok(b.finish());
        }
        return Err(format!("bad body handle `{}`", handle));
    }

    let set = LoadedSet::new(budget);
    let items = set.axis_items(store, &target, &axis)?;
    let sliced: Vec<String> = items.into_iter().skip(start).collect();
    b.push_items(&target, &axis, &sliced);
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
            DocRecord {
                content_hash: hash_hex(text),
                sections: crate::md::parse_sections(text),
                coverage: BTreeMap::new(),
            },
        );
        s.graph.entities.insert(
            "ent:shopping-cart".into(),
            Entity {
                name: "Shopping Cart".into(),
                definition: Some("holds items a customer intends to buy".into()),
                mentions: vec![SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop/cart".into(),
                    quote: "The Shopping Cart holds items.".into(),
                }],
                ..Default::default()
            },
        );
        s.graph.entities.insert(
            "ent:customer".into(),
            Entity {
                name: "Customer".into(),
                definition: Some("a person who buys".into()),
                ..Default::default()
            },
        );
        for i in 0..6 {
            s.graph.requirements.insert(
                format!("req:shop-{}", i + 1),
                Requirement {
                    statement: format!(
                        "When event {} happens, the system shall update the Shopping Cart.",
                        i + 1
                    ),
                    entities: vec!["ent:shopping-cart".into(), "ent:customer".into()],
                    edges: vec![],
                    source: Some(SourceRef {
                        doc: "shop.md".into(),
                        section: "/shop/cart".into(),
                        quote: "holds items".into(),
                    }),
                    ..Default::default()
                },
            );
        }
        s
    }

    #[test]
    fn entity_pack_within_budget_with_handles() {
        let s = fixture();
        let pack = assemble(&s, "ent:shopping-cart", &Focus::default(), 700).unwrap();
        assert!(pack.pack.contains("Entity ent:shopping-cart"));
        assert!(
            !pack.handles.is_empty(),
            "small budget should cut requirements into a handle"
        );
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

    // The handle grammar parses from the right: axis names are a closed set, targets
    // carry colons of their own. Mirrors docs/compiler/context.md#expansion-handles.
    #[test]
    fn handle_grammar_parses_from_the_right() {
        assert_eq!(
            parse_handle("h:ent:shopping-cart:requirements").unwrap(),
            ("ent:shopping-cart".into(), "requirements".into(), 0)
        );
        assert_eq!(
            parse_handle("h:ent:shopping-cart:requirements:4").unwrap(),
            ("ent:shopping-cart".into(), "requirements".into(), 4)
        );
        assert_eq!(
            parse_handle("h:view:class/commerce:members").unwrap(),
            ("view:class/commerce".into(), "members".into(), 0)
        );
        assert_eq!(
            parse_handle("h:ent:customer").unwrap(),
            ("ent:customer".into(), "".into(), 0)
        );
        assert_eq!(
            parse_handle("h:shop.md#/shop/cart:body:120").unwrap(),
            ("shop.md#/shop/cart".into(), "body".into(), 120)
        );
        assert!(parse_handle("x:nope").is_err());
    }

    // A load that exceeds the budget emits handles; expand pulls the frontier and the
    // continuation handle picks up where the slice ended; unload frees the budget.
    #[test]
    fn loaded_set_budget_handles_expand_and_unload() {
        let s = fixture();
        let mut set = LoadedSet::new(520);
        let text = set.load(&s, "ent:shopping-cart", 1).unwrap();
        assert!(text.contains("ent:shopping-cart"));
        let handles = set.open_handles();
        assert!(
            handles.iter().any(|h| h.contains("requirements")),
            "cut requirements into a handle: {:?}",
            handles
        );
        let h = handles
            .iter()
            .find(|h| h.contains("requirements"))
            .unwrap()
            .clone();
        let before = set.used();
        let expansion = set.expand(&s, &h).unwrap();
        assert!(expansion.contains("req:shop-"));
        assert!(
            set.used() > before,
            "an expansion counts against the budget"
        );
        assert!(set.unload("ent:shopping-cart"));
        assert_eq!(set.used(), 0, "unload frees the budget");
        assert!(set.open_handles().is_empty(), "handles close with the item");
        let err = set.expand(&s, &h).unwrap_err();
        assert!(err.contains("unknown or closed handle"), "{}", err);
    }

    #[test]
    fn dirty_section_marks_the_diff_or_says_unavailable() {
        let mut s = fixture();
        s.status.changes.push(ChangeRecord::new(
            1,
            1,
            0,
            "section-dirty",
            "shop.md#/shop/cart",
            "section",
        ));
        let mut set = LoadedSet::new(20_000);
        let text = set.load(&s, "shop.md#/shop/cart", 1).unwrap();
        assert!(
            text.contains("previous body unavailable") || text.contains("+ "),
            "{}",
            text
        );
    }
}
