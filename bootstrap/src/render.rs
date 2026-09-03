// Diagrams: one emitter per view kind, each a pure function of the store snapshot and
// the view, producing PlantUML text; lifting and collapse over the containment tree;
// over-limit auto-collapse; the render seam; the files under <out>/diagrams/.
// Mirrors docs/compiler/diagrams.md.
use crate::derive::{entity_slug, instance_types, normalize_state, query_matches};
use crate::limits;
use crate::model::*;
use crate::store::Store;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Debug, PartialEq)]
pub struct RenderError(pub String);

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RenderError {}

// ---- the seam ----

// PlantUML text to SVG. In process through plantuml-little; the official native binary
// named by JAZYK_PLANTUML is the authorized swap. Mirrors docs/compiler/diagrams.md#the-renderer.
pub fn render_svg(puml: &str) -> Result<String, RenderError> {
    match std::env::var("JAZYK_PLANTUML") {
        Ok(bin) if !bin.trim().is_empty() => native_svg(bin.trim(), puml),
        _ => in_process_svg(puml),
    }
}

// A panic inside the crate is a renderer defect like any other error: it names itself
// and never takes the build down. The crate reads a `[[...]]` on an element declaration
// as part of the name (a class takes its alias as its label, an actor splits in two)
// and never emits an anchor, so the links come off before conversion and go back on
// as anchors after; the official binary behind JAZYK_PLANTUML does both itself.
fn in_process_svg(puml: &str) -> Result<String, RenderError> {
    let (plain, links) = strip_element_links(puml);
    match std::panic::catch_unwind(|| plantuml_little::convert(&plain)) {
        Ok(Ok(svg)) => Ok(anchor_entities(&svg, &links)),
        Ok(Err(e)) => Err(RenderError(e.to_string())),
        Err(panic) => {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "no message".to_string());
            Err(RenderError(format!("renderer panicked: {}", message)))
        }
    }
}

fn native_svg(bin: &str, puml: &str) -> Result<String, RenderError> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let fail = |e: &dyn fmt::Display| RenderError(format!("{}: {}", bin, e));
    let mut child = Command::new(bin)
        .args(["-tsvg", "-pipe"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| fail(&e))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A child that dies without reading (or before the write finishes) breaks the
        // pipe; its exit status below is the story, so the write tolerates that one.
        if let Err(e) = stdin.write_all(puml.as_bytes()) {
            if e.kind() != std::io::ErrorKind::BrokenPipe {
                return Err(fail(&e));
            }
        }
    }
    let out = child.wait_with_output().map_err(|e| fail(&e))?;
    if !out.status.success() {
        return Err(RenderError(format!(
            "{} exited with {}: {}",
            bin,
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    String::from_utf8(out.stdout).map_err(|e| fail(&e))
}

// The system fonts, loaded once per process: text measures as a viewer would draw it.
fn fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

// SVG to PNG bytes through resvg.
pub fn render_png(svg: &str) -> Result<Vec<u8>, RenderError> {
    let mut opt = resvg::usvg::Options::default();
    opt.fontdb = fonts();
    let tree = resvg::usvg::Tree::from_str(svg, &opt).map_err(|e| RenderError(e.to_string()))?;
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| RenderError("the svg has no size".to_string()))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|e| RenderError(e.to_string()))
}

// ---- text helpers ----

fn oneline(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", oneline(name).replace('"', "'"))
}

// Words PlantUML reads as syntax at the start of a line; an alias never spells one.
const PLANTUML_WORDS: [&str; 64] = [
    "abstract",
    "activate",
    "actor",
    "agent",
    "alt",
    "artifact",
    "as",
    "boundary",
    "box",
    "break",
    "caption",
    "card",
    "class",
    "cloud",
    "collections",
    "component",
    "control",
    "create",
    "database",
    "deactivate",
    "destroy",
    "diamond",
    "else",
    "end",
    "endif",
    "endwhile",
    "entity",
    "enum",
    "file",
    "folder",
    "footer",
    "fork",
    "frame",
    "group",
    "header",
    "hexagon",
    "hide",
    "if",
    "interface",
    "is",
    "label",
    "legend",
    "loop",
    "namespace",
    "node",
    "note",
    "object",
    "opt",
    "package",
    "par",
    "participant",
    "person",
    "queue",
    "rectangle",
    "ref",
    "repeat",
    "return",
    "show",
    "skinparam",
    "start",
    "state",
    "stop",
    "title",
    "usecase",
];

fn identifier(base: &str, prefix: &str) -> String {
    let id: String = base
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    if id.is_empty()
        || id.starts_with(|c: char| c.is_ascii_digit())
        || PLANTUML_WORDS.contains(&id.as_str())
    {
        format!("{}{}", prefix, id)
    } else {
        id
    }
}

// The PlantUML alias of an entity: derived from its id, stable across builds.
fn alias_of(id: &str) -> String {
    identifier(entity_slug(id), "n_")
}

// A state name usable as a PlantUML token, or the alias it needs a declaration for.
fn state_token(name: &str) -> (Option<String>, String) {
    let flat = oneline(name);
    let plain = !flat.is_empty()
        && flat.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !flat.starts_with(|c: char| c.is_ascii_digit())
        && !PLANTUML_WORDS.contains(&flat.as_str());
    if plain {
        (None, flat)
    } else {
        let alias = identifier(&flat.to_lowercase(), "s_");
        (Some(format!("state {} as {}", quoted(&flat), alias)), alias)
    }
}

fn document(title: Option<String>, body: Vec<String>) -> String {
    let mut s = String::from("@startuml\n");
    if let Some(t) = title {
        s.push_str("title ");
        s.push_str(&t);
        s.push('\n');
    }
    for line in body {
        s.push_str(&line);
        s.push('\n');
    }
    s.push_str("@enduml\n");
    s
}

fn empty_diagram(view: &View, what: &str) -> String {
    document(
        None,
        vec![format!(
            "rectangle {} as empty",
            quoted(&format!("{}: {}", view.title, what))
        )],
    )
}

fn notation(rel_type: &str) -> &'static str {
    match rel_type {
        "generalization" => "--|>",
        "realization" => "..|>",
        "composition" => "*--",
        "aggregation" => "o--",
        "association" => "--",
        _ => "..>",
    }
}

// ---- the scene: a view resolved against the snapshot ----

struct Scene<'a> {
    store: &'a Store,
    id: &'a str,
    view: &'a View,
    // Entity members, in member order, query matches joined, exclusions removed.
    entities: Vec<String>,
    // Requirement members, in member order.
    requirements: Vec<String>,
}

fn scene<'a>(store: &'a Store, id: &'a str, view: &'a View) -> Scene<'a> {
    let instances = instance_types(store);
    let mut ids: Vec<String> = Vec::new();
    for m in &view.members {
        let r = store.resolve_id(m).to_string();
        if !ids.contains(&r) {
            ids.push(r);
        }
    }
    if let Some(q) = view.query.as_ref() {
        for m in query_matches(store, q, &view.excluded, &instances) {
            if !ids.contains(&m) {
                ids.push(m);
            }
        }
    }
    let excluded: BTreeSet<String> = view
        .excluded
        .iter()
        .map(|x| store.resolve_id(&x.id).to_string())
        .collect();
    ids.retain(|m| !excluded.contains(m));
    let entities = ids
        .iter()
        .filter(|m| store.graph.entities.contains_key(*m))
        .cloned()
        .collect();
    let requirements = ids
        .iter()
        .filter(|m| store.graph.requirements.contains_key(*m))
        .cloned()
        .collect();
    Scene {
        store,
        id,
        view,
        entities,
        requirements,
    }
}

impl Scene<'_> {
    fn entity(&self, id: &str) -> &Entity {
        &self.store.graph.entities[id]
    }

    fn name(&self, id: &str) -> String {
        oneline(&self.entity(id).name)
    }

    fn labeled(&self, id: &str, stereotype: &str) -> bool {
        self.entity(id)
            .stereotype
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case(stereotype))
    }

    fn requirement(&self, id: &str) -> &Requirement {
        &self.store.graph.requirements[id]
    }

    // A flow member's label: its statement followed by its id.
    fn flow_label(&self, rid: &str) -> String {
        format!("{} ({})", oneline(&self.requirement(rid).statement), rid)
    }

    fn machine(&self, entity: &str) -> Option<&StateMachine> {
        self.store
            .graph
            .state_machines
            .get(&format!("sm:{}", entity_slug(entity)))
    }
}

// ---- containment ----

// The entity and its ancestors, nearest first. Bounded against a cycle.
fn ancestry(store: &Store, id: &str) -> Vec<String> {
    let mut chain = vec![id.to_string()];
    let mut cur = id.to_string();
    while let Some(p) = store
        .graph
        .entities
        .get(&cur)
        .and_then(|e| e.parent.as_deref())
    {
        let p = store.resolve_id(p).to_string();
        if chain.contains(&p) || chain.len() > 64 {
            break;
        }
        chain.push(p.clone());
        cur = p;
    }
    chain
}

fn is_below(store: &Store, ancestor: &str, id: &str) -> bool {
    ancestry(store, id).iter().skip(1).any(|x| x == ancestor)
}

// The shown node standing for an entity: the topmost collapsed member on its chain,
// else the nearest member on its chain, else none (not drawn).
fn representative(
    store: &Store,
    members: &BTreeSet<String>,
    collapse: &BTreeSet<String>,
    id: &str,
) -> Option<String> {
    let chain = ancestry(store, id);
    if let Some(c) = chain
        .iter()
        .rev()
        .find(|x| collapse.contains(*x) && members.contains(*x))
    {
        return Some(c.clone());
    }
    chain.into_iter().find(|x| members.contains(x))
}

// ---- lifting and collapse ----

// One drawn arrow: the strongest type among the concrete groups beneath it, the
// cardinality when one group stands alone, and the count of groups.
struct Arrow {
    a: String,
    b: String,
    rel_type: String,
    cardinality: Option<String>,
    count: usize,
}

struct Structure {
    // The drawn entities, in member order.
    shown: Vec<String>,
    // The drawn entities hiding a subtree.
    collapsed: BTreeSet<String>,
    arrows: Vec<Arrow>,
}

fn pair_key(a: &str, b: &str) -> String {
    let (x, y) = if a <= b { (a, b) } else { (b, a) };
    format!("{}~{}", entity_slug(x), entity_slug(y))
}

// The members minus the subtrees hidden by collapse, and every relationship among them,
// direct or lifted, one arrow per shown pair and direction.
// Mirrors docs/compiler/diagrams.md#lifting-and-collapse.
fn structure(store: &Store, members: &[String], collapse: &BTreeSet<String>) -> Structure {
    let set: BTreeSet<String> = members.iter().cloned().collect();
    let rep = |id: &str| representative(store, &set, collapse, id);
    let shown: Vec<String> = members
        .iter()
        .filter(|m| rep(m).as_deref() == Some(m.as_str()))
        .cloned()
        .collect();
    let collapsed: BTreeSet<String> = shown
        .iter()
        .filter(|s| collapse.contains(*s))
        .cloned()
        .collect();
    let mut groups: BTreeMap<(String, String), Vec<&Contribution>> = BTreeMap::new();
    for rel in store.graph.relationships.values() {
        for c in &rel.contributions {
            if c.r#type == INSTANTIATION {
                continue;
            }
            let (Some(a), Some(b)) = (rep(&c.a), rep(&c.b)) else {
                continue;
            };
            if a == b {
                continue;
            }
            groups.entry((a, b)).or_default().push(c);
        }
    }
    let mut arrows: Vec<Arrow> = groups
        .into_iter()
        .map(|((a, b), cs)| {
            let rel_type = cs
                .iter()
                .min_by_key(|c| rel_rank(&c.r#type))
                .map(|c| c.r#type.clone())
                .unwrap_or_else(|| DEFAULT_REL_TYPE.to_string());
            let cardinality = if cs.len() == 1 {
                cs[0].cardinality.clone()
            } else {
                None
            };
            Arrow {
                a,
                b,
                rel_type,
                cardinality,
                count: cs.len(),
            }
        })
        .collect();
    arrows.sort_by(|x, y| {
        (pair_key(&x.a, &x.b), &x.a, &x.b).cmp(&(pair_key(&y.a, &y.b), &y.a, &y.b))
    });
    Structure {
        shown,
        collapsed,
        arrows,
    }
}

// ---- over-limit views ----

fn limit_pair(view: &View, limit: &str) -> (u64, u64) {
    limits::threshold(limit, view.limits.get(limit).map(|b| b.value))
        .unwrap_or((u64::MAX, u64::MAX))
}

fn structural_counts(kind: &str, st: &Structure) -> Vec<(&'static str, u64)> {
    let members = if kind == "object" {
        "instances-per-object-view"
    } else {
        "members-per-structural-view"
    };
    vec![
        (members, st.shown.len() as u64),
        ("edges-per-view", st.arrows.len() as u64),
    ]
}

struct Frame {
    st: Structure,
    note: Option<String>,
}

// The structure of a view with its authored collapse, then auto-collapse of the largest
// subtrees while a count is past its hard threshold, until every count is within the
// soft one or nothing is left to collapse. Mirrors docs/compiler/diagrams.md#over-limit-views.
fn frame(scene: &Scene) -> Frame {
    let store = scene.store;
    let view = scene.view;
    let kind = view.kind.as_str();
    let mut collapse: BTreeSet<String> = view
        .collapse
        .iter()
        .map(|c| store.resolve_id(c).to_string())
        .collect();
    let mut st = structure(store, &scene.entities, &collapse);
    let over_hard = |st: &Structure| {
        structural_counts(kind, st)
            .into_iter()
            .find(|(l, n)| *n > limit_pair(view, l).1)
    };
    let Some(first) = over_hard(&st) else {
        return Frame { st, note: None };
    };
    let mut auto = 0usize;
    loop {
        if structural_counts(kind, &st)
            .iter()
            .all(|(l, n)| *n <= limit_pair(view, l).0)
        {
            break;
        }
        let mut best: Option<(String, usize)> = None;
        for s in &st.shown {
            if collapse.contains(s) {
                continue;
            }
            let below = st
                .shown
                .iter()
                .filter(|o| *o != s && is_below(store, s, o))
                .count();
            if below > 0 && best.as_ref().map_or(true, |(_, n)| below > *n) {
                best = Some((s.clone(), below));
            }
        }
        let Some((node, _)) = best else {
            break;
        };
        collapse.insert(node);
        auto += 1;
        st = structure(store, &scene.entities, &collapse);
    }
    let note = if auto > 0 {
        format!("(collapsed: {} subtrees over limit)", auto)
    } else {
        let (limit, n) = first;
        let what = if limit == "edges-per-view" {
            "edges"
        } else {
            "members"
        };
        format!("(over limit: {} {})", n, what)
    };
    Frame {
        st,
        note: Some(note),
    }
}

// A flow view past its hard member or participant threshold renders every member,
// marked.
fn flow_note(scene: &Scene, participants: Option<usize>) -> Option<String> {
    let n = scene.requirements.len() as u64;
    if n > limit_pair(scene.view, "members-per-flow-view").1 {
        return Some(format!("(over limit: {} members)", n));
    }
    if let Some(p) = participants {
        if p as u64 > limit_pair(scene.view, "participants-per-sequence-view").1 {
            return Some(format!("(over limit: {} participants)", p));
        }
    }
    None
}

fn title_line(view: &View, note: Option<&str>) -> Option<String> {
    note.map(|n| format!("{} {}", oneline(&view.title), n))
}

// ---- drill-down links ----

// The PlantUML hyperlink to a view's rendering, relative under diagrams/: from any
// diagrams/<kind>/<slug>.svg, `../<kind>/<slug>.svg` is the sibling tree.
// Mirrors docs/compiler/diagrams.md#output-layout.
fn view_link(view_id: &str) -> String {
    format!(
        "[[../{}.svg]]",
        view_id.strip_prefix("view:").unwrap_or(view_id)
    )
}

// The link a rendered member carries down to its level view, when its entity has one
// and it is not the view being drawn. Mirrors docs/compiler/diagrams.md#drill-down.
fn level_link(scene: &Scene, id: &str) -> Option<String> {
    crate::derive::level_view_id(scene.store, id)
        .filter(|v| v != scene.id)
        .map(|v| view_link(&v))
}

// The view of the same kind detailing a collapsed node: the one whose query.parent is
// the node, else the one whose members all sit below the node.
// Mirrors docs/compiler/diagrams.md#lifting-and-collapse.
fn sub_view_link(scene: &Scene, node: &str) -> Option<String> {
    let store = scene.store;
    let candidates = || {
        store
            .graph
            .views
            .iter()
            .filter(|(id, v)| *id != scene.id && v.kind == scene.view.kind)
    };
    let by_query = candidates().find(|(_, v)| {
        v.query
            .as_ref()
            .and_then(|q| q.parent.as_deref())
            .is_some_and(|p| store.resolve_id(p) == node)
    });
    let by_members = || {
        candidates().find(|(_, v)| {
            !v.members.is_empty()
                && v.members
                    .iter()
                    .all(|m| is_below(store, node, store.resolve_id(m)))
        })
    };
    by_query.or_else(by_members).map(|(id, _)| view_link(id))
}

// A member links to its level view whether collapsed or not; a collapsed node without
// one links to the sub-view detailing it, and a collapsed node with both keeps the
// curated sub-view's precedence. Mirrors docs/compiler/diagrams.md#drill-down.
fn link_suffix(scene: &Scene, st: &Structure, node: &str) -> String {
    let link = if st.collapsed.contains(node) {
        sub_view_link(scene, node).or_else(|| level_link(scene, node))
    } else {
        level_link(scene, node)
    };
    link.map(|l| format!(" {}", l)).unwrap_or_default()
}

// The links on element declarations, taken off the text: `(line without the link,
// [(element key, href)])`. A link is a ` [[../...]]` on a declaration line; the key is
// the alias after ` as `, or the quoted name of a package declared without one, the
// same name the crate writes as the element's `data-qualified-name`.
fn strip_element_links(puml: &str) -> (String, Vec<(String, String)>) {
    let mut plain = String::with_capacity(puml.len());
    let mut links: Vec<(String, String)> = Vec::new();
    for line in puml.lines() {
        let stripped = line.find(" [[../").and_then(|start| {
            let after = &line[start + 3..];
            let end = after.find("]]")?;
            let href = &after[..end];
            let head = &line[..start];
            let tail = &after[end + 2..];
            let key = match head.rsplit_once(" as ") {
                Some((_, alias)) => alias.split_whitespace().next()?.to_string(),
                None => head
                    .split_once(' ')
                    .map(|(_, name)| name.trim().trim_matches('"').to_string())?,
            };
            Some((format!("{}{}", head, tail), key, href.to_string()))
        });
        match stripped {
            Some((clean, key, href)) => {
                plain.push_str(&clean);
                links.push((key, href));
            }
            None => plain.push_str(line),
        }
        plain.push('\n');
    }
    (plain, links)
}

fn xml_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// Every `<g class="entity" data-qualified-name="<key>" ...>...</g>` the crate drew for a
// linked element, wrapped in `<a href>`: the whole shape is the click target, as the
// official renderer draws it. An element the crate did not draw under that key keeps
// no link; nothing else in the document changes.
fn anchor_entities(svg: &str, links: &[(String, String)]) -> String {
    let mut out = svg.to_string();
    for (key, href) in links {
        let marker = format!(
            "<g class=\"entity\" data-qualified-name=\"{}\"",
            xml_attr(key)
        );
        let mut from = 0;
        while let Some(rel) = out[from..].find(&marker) {
            let start = from + rel;
            let Some(end) = group_end(&out, start) else {
                break;
            };
            let anchor = format!("<a href=\"{0}\" xlink:href=\"{0}\">", xml_attr(href));
            out.insert_str(end, "</a>");
            out.insert_str(start, &anchor);
            from = end + anchor.len() + "</a>".len();
        }
    }
    out
}

// The index just past the `</g>` closing the group opened at `start`, nesting counted.
fn group_end(svg: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = start;
    while i < svg.len() {
        let rest = &svg[i..];
        if rest.starts_with("<g ") || rest.starts_with("<g>") {
            depth += 1;
            i += 2;
        } else if rest.starts_with("</g>") {
            depth -= 1;
            i += 4;
            if depth == 0 {
                return Some(i);
            }
        } else {
            i += rest.chars().next()?.len_utf8();
        }
    }
    None
}

// `keyword "Name" as alias [<<stereotype>>] [[[link]]]`
fn declare(scene: &Scene, st: &Structure, keyword: &str, id: &str, stereotype: bool) -> String {
    let e = scene.entity(id);
    let mut line = format!("{} {} as {}", keyword, quoted(&e.name), alias_of(id));
    // The stereotype rides on the element unless it is the keyword it became (a
    // «component» drawn as `component` says nothing twice). Mirrors
    // docs/compiler/diagrams.md#the-emitters.
    if stereotype {
        if let Some(s) = e.stereotype.as_deref() {
            if !s.eq_ignore_ascii_case(keyword) {
                line.push_str(&format!(" <<{}>>", oneline(s)));
            }
        }
    }
    line.push_str(&link_suffix(scene, st, id));
    line
}

fn with_body(out: &mut Vec<String>, line: String, body: Vec<String>) {
    if body.is_empty() {
        out.push(line);
        return;
    }
    out.push(format!("{} {{", line));
    for b in body {
        out.push(format!("  {}", b));
    }
    out.push("}".to_string());
}

fn arrow_line(a: &Arrow, notation: &str, label: Option<&str>) -> String {
    arrow_line_with(a, notation, label, &|id| alias_of(id))
}

// The count label stays ASCII: the crate measures edge labels byte by byte.
fn arrow_line_with(
    a: &Arrow,
    notation: &str,
    label: Option<&str>,
    node: &dyn Fn(&str) -> String,
) -> String {
    let mut s = format!("{} {} ", node(&a.a), notation);
    if let Some(c) = a.cardinality.as_deref() {
        s.push_str(&format!("{} ", quoted(c)));
    }
    s.push_str(&node(&a.b));
    let mut labels: Vec<String> = Vec::new();
    if let Some(l) = label {
        labels.push(l.to_string());
    }
    if a.count > 1 {
        labels.push(format!("{} edges", a.count));
    }
    if !labels.is_empty() {
        s.push_str(&format!(" : {}", labels.join(" ")));
    }
    s
}

fn arrow_lines(arrows: &[Arrow]) -> Vec<String> {
    arrows
        .iter()
        .map(|a| arrow_line(a, notation(&a.rel_type), None))
        .collect()
}

// `name : type` when the type is stated, `name = value` when a value is, else `name`.
fn attribute_line(a: &Attribute) -> String {
    match (a.r#type.as_deref(), a.value.as_deref()) {
        (Some(t), _) => format!("{} : {}", oneline(&a.name), oneline(t)),
        (None, Some(v)) => format!("{} = {}", oneline(&a.name), oneline(v)),
        (None, None) => oneline(&a.name),
    }
}

// ---- structural emitters ----

fn emit_class(scene: &Scene) -> String {
    let f = frame(scene);
    let mut out = Vec::new();
    for id in &f.st.shown {
        let line = declare(scene, &f.st, "class", id, true);
        let body = scene
            .entity(id)
            .attributes
            .iter()
            .map(attribute_line)
            .collect();
        with_body(&mut out, line, body);
    }
    out.extend(arrow_lines(&f.st.arrows));
    document(title_line(scene.view, f.note.as_deref()), out)
}

fn emit_object(scene: &Scene) -> String {
    let f = frame(scene);
    let types = instance_types(scene.store);
    let mut out = Vec::new();
    for id in &f.st.shown {
        let e = scene.entity(id);
        let label = match types
            .get(id)
            .and_then(|t| scene.store.graph.entities.get(t))
        {
            Some(t) => format!("{} : {}", oneline(&e.name), oneline(&t.name)),
            None => oneline(&e.name),
        };
        let mut line = format!("object {} as {}", quoted(&label), alias_of(id));
        line.push_str(&link_suffix(scene, &f.st, id));
        let body = e
            .attributes
            .iter()
            .map(|a| match (a.value.as_deref(), a.r#type.as_deref()) {
                (Some(v), _) => format!("{} = {}", oneline(&a.name), oneline(v)),
                (None, Some(t)) => format!("{} : {}", oneline(&a.name), oneline(t)),
                (None, None) => oneline(&a.name),
            })
            .collect();
        with_body(&mut out, line, body);
    }
    out.extend(arrow_lines(&f.st.arrows));
    document(title_line(scene.view, f.note.as_deref()), out)
}

fn emit_package(scene: &Scene) -> String {
    let store = scene.store;
    let f = frame(scene);
    let shown = &f.st.shown;
    let set: BTreeSet<&str> = shown.iter().map(String::as_str).collect();
    let parent_in = |id: &str| -> Option<String> {
        scene
            .entity(id)
            .parent
            .as_deref()
            .map(|p| store.resolve_id(p).to_string())
            .filter(|p| set.contains(p.as_str()))
    };
    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for s in shown {
        if let Some(p) = parent_in(s) {
            children.entry(p).or_default().push(s.clone());
        }
    }
    let roots: Vec<String> = shown
        .iter()
        .filter(|s| parent_in(s).is_none())
        .cloned()
        .collect();
    let mut out = Vec::new();
    // Packages go by their quoted name, never an alias: the crate emits invalid DOT for
    // an aliased package with a body. Classes inside keep their aliases.
    // A scope query with no containment among the members: the scope is the package.
    if children.is_empty() && !shown.is_empty() {
        if let Some(scope) = scene.view.query.as_ref().and_then(|q| q.scope.as_deref()) {
            out.push(format!("package {} {{", quoted(scope)));
            for s in shown {
                out.push(format!("  {}", declare(scene, &f.st, "class", s, true)));
            }
            out.push("}".to_string());
            return document(title_line(scene.view, f.note.as_deref()), out);
        }
    }
    fn emit_node(
        out: &mut Vec<String>,
        indent: usize,
        id: &str,
        children: &BTreeMap<String, Vec<String>>,
        scene: &Scene,
        st: &Structure,
    ) {
        let pad = "  ".repeat(indent);
        let name = quoted(&scene.entity(id).name);
        match children.get(id) {
            Some(kids) if !kids.is_empty() => {
                out.push(format!("{}package {} {{", pad, name));
                for k in kids {
                    if children.get(k).is_some_and(|c| !c.is_empty()) {
                        emit_node(out, indent + 1, k, children, scene, st);
                    } else {
                        out.push(format!("{}  {}", pad, declare(scene, st, "class", k, true)));
                    }
                }
                out.push(format!("{}}}", pad));
            }
            // Never an empty body: the one-line form.
            _ => out.push(format!(
                "{}package {}{}",
                pad,
                name,
                link_suffix(scene, st, id)
            )),
        }
    }
    for r in &roots {
        emit_node(&mut out, 0, r, &children, scene, &f.st);
    }
    // Arrows between top-level packages only: everything lifts to its root.
    let root_set: BTreeSet<String> = roots.iter().cloned().collect();
    let lifted = structure(store, shown, &root_set);
    let by_name = |id: &str| quoted(&scene.name(id));
    for a in &lifted.arrows {
        out.push(arrow_line_with(a, notation(&a.rel_type), None, &by_name));
    }
    document(title_line(scene.view, f.note.as_deref()), out)
}

fn emit_component(scene: &Scene) -> String {
    let f = frame(scene);
    let mut out = Vec::new();
    for id in &f.st.shown {
        let keyword = if scene.labeled(id, "actor") {
            "actor"
        } else if scene.labeled(id, "interface") {
            "interface"
        } else {
            "component"
        };
        out.push(declare(scene, &f.st, keyword, id, true));
    }
    for a in &f.st.arrows {
        let to_interface = scene.labeled(&a.b, "interface");
        let line = if a.rel_type == "realization" && to_interface {
            arrow_line(a, "--", None)
        } else if a.rel_type == "dependency" && to_interface && !scene.labeled(&a.a, "actor") {
            arrow_line(a, "--(", Some("use"))
        } else {
            arrow_line(a, notation(&a.rel_type), None)
        };
        out.push(line);
    }
    document(title_line(scene.view, f.note.as_deref()), out)
}

fn emit_composite(scene: &Scene) -> String {
    let store = scene.store;
    let f = frame(scene);
    let shown = &f.st.shown;
    let boundary = shown
        .iter()
        .find(|b| shown.iter().any(|o| o != *b && is_below(store, b, o)))
        .or(shown.first())
        .cloned();
    let Some(boundary) = boundary else {
        return empty_diagram(scene.view, "no boundary entity");
    };
    let (parts, outside): (Vec<&String>, Vec<&String>) = shown
        .iter()
        .filter(|o| **o != boundary)
        .partition(|o| is_below(store, &boundary, o));
    let mut out = Vec::new();
    out.push(format!(
        "component {} as {} {{",
        quoted(&scene.name(&boundary)),
        alias_of(&boundary)
    ));
    for p in &parts {
        out.push(format!("  [{}] as {}", scene.name(p), alias_of(p)));
    }
    out.push("}".to_string());
    for o in &outside {
        out.push(format!("[{}] as {}", scene.name(o), alias_of(o)));
    }
    for a in &f.st.arrows {
        if a.a == boundary || a.b == boundary {
            continue;
        }
        out.push(arrow_line(a, notation(&a.rel_type), None));
    }
    document(title_line(scene.view, f.note.as_deref()), out)
}

fn emit_deployment(scene: &Scene) -> String {
    let f = frame(scene);
    let mut placed: Vec<(String, Vec<String>)> = Vec::new();
    let mut bare: Vec<String> = Vec::new();
    for id in &f.st.shown {
        match scene
            .entity(id)
            .attributes
            .iter()
            .find(|a| a.value.is_some())
        {
            Some(a) => {
                let label = format!(
                    "{} {}",
                    oneline(a.value.as_deref().unwrap_or_default()),
                    oneline(&a.name)
                );
                match placed.iter_mut().find(|(l, _)| *l == label) {
                    Some((_, arts)) => arts.push(id.clone()),
                    None => placed.push((label, vec![id.clone()])),
                }
            }
            None => bare.push(id.clone()),
        }
    }
    let mut out = Vec::new();
    for (label, arts) in &placed {
        out.push(format!(
            "node {} as {} {{",
            quoted(label),
            identifier(&label.to_lowercase(), "p_")
        ));
        for id in arts {
            out.push(format!(
                "  {}",
                declare(scene, &f.st, "artifact", id, false)
            ));
        }
        out.push("}".to_string());
    }
    for id in &bare {
        out.push(declare(scene, &f.st, "artifact", id, false));
    }
    out.extend(arrow_lines(&f.st.arrows));
    document(title_line(scene.view, f.note.as_deref()), out)
}

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
fn message_of(store: &Store, rid: &str) -> Option<Message> {
    let r = store.graph.requirements.get(rid)?;
    let live = |id: &str| -> Option<String> {
        let id = store.resolve_id(id).to_string();
        store.graph.entities.contains_key(&id).then_some(id)
    };
    let edge = r
        .edges
        .iter()
        .find(|e| e.rel_type.as_deref().map_or(true, |t| t == "dependency"))
        .or_else(|| r.edges.first());
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
    let level = crate::derive::flow_view_level(scene.store, scene.id)
        .map(|t| crate::derive::level_view_members(scene.store, &t));
    let lift = |id: String| -> String {
        match level.as_deref() {
            Some(members) => {
                crate::derive::lift_into(scene.store, members, &id).unwrap_or(id)
            }
            None => id,
        }
    };
    raw_messages(scene)
        .into_iter()
        .map(|m| Message {
            from: lift(m.from),
            to: lift(m.to),
            rid: m.rid,
        })
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
    if actors.is_empty() {
        let receivers: BTreeSet<&str> = messages.iter().map(|m| m.to.as_str()).collect();
        for m in messages {
            if !receivers.contains(m.from.as_str()) && !actors.contains(&m.from) {
                actors.push(m.from.clone());
            }
        }
    }
    actors
}

fn use_case_alias(scene: &Scene) -> String {
    let slug = scene.id.rsplit('/').next().unwrap_or(scene.id);
    identifier(slug, "uc_")
}

fn emit_use_case(scene: &Scene) -> String {
    let msgs = messages(scene);
    let note = flow_note(scene, None);
    let uc = use_case_alias(scene);
    let mut out = Vec::new();
    let actors = flow_actors(scene, &msgs);
    // An actor holding a level of its own links down to it like any rendered member.
    for a in &actors {
        let link = level_link(scene, a)
            .map(|l| format!(" {}", l))
            .unwrap_or_default();
        out.push(format!(
            "actor {} as {}{}",
            quoted(&scene.name(a)),
            alias_of(a),
            link
        ));
    }
    out.push(format!("usecase {} as {}", quoted(&scene.view.title), uc));
    for a in &actors {
        out.push(format!("{} -- {}", alias_of(a), uc));
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

fn emit_activity(scene: &Scene) -> String {
    let note = flow_note(scene, None);
    let mut out = vec!["start".to_string()];
    out.extend(activity_steps(scene, &scene.requirements));
    out.push("stop".to_string());
    document(title_line(scene.view, note.as_deref()), out)
}

fn emit_state(scene: &Scene) -> String {
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

fn emit_sequence(scene: &Scene) -> String {
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
        out.push(format!(
            "{} {} as {}",
            keyword,
            quoted(&scene.name(p)),
            alias_of(p)
        ));
    }
    for m in &msgs {
        out.push(format!(
            "{} -> {} : {}",
            alias_of(&m.from),
            alias_of(&m.to),
            scene.flow_label(&m.rid)
        ));
    }
    document(title_line(scene.view, note.as_deref()), out)
}

fn emit_communication(scene: &Scene) -> String {
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
            "{} {} as {}",
            keyword,
            quoted(&scene.name(p)),
            alias_of(p)
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

fn emit_timing(scene: &Scene) -> String {
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

fn emit_overview(scene: &Scene) -> String {
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

// ---- dispatch ----

// The PlantUML text of a view over the snapshot. Deterministic: the same snapshot and
// view produce byte-identical output. Mirrors docs/compiler/diagrams.md#the-emitters.
pub fn emit(store: &Store, view_id: &str, view: &View) -> String {
    let scene = scene(store, view_id, view);
    let puml = emit_kind(&scene);
    // A diagram with nothing in it is not a diagram: draw the fact instead.
    let blank = puml
        .lines()
        .all(|l| l == "@startuml" || l == "@enduml" || l.starts_with("title "));
    if blank {
        return empty_diagram(view, "no members");
    }
    puml
}

fn emit_kind(scene: &Scene) -> String {
    let view = scene.view;
    match view.kind.as_str() {
        "class" => emit_class(scene),
        "object" => emit_object(scene),
        "package" => emit_package(scene),
        "component" => emit_component(scene),
        "composite" => emit_composite(scene),
        "deployment" => emit_deployment(scene),
        "use-case" => emit_use_case(scene),
        "activity" => emit_activity(scene),
        "state" => emit_state(scene),
        "sequence" => emit_sequence(scene),
        "communication" => emit_communication(scene),
        "timing" => emit_timing(scene),
        "overview" => emit_overview(scene),
        other => empty_diagram(view, &format!("view kind {} has no emitter", other)),
    }
}

// ---- output ----

// Every view, plus a state view per machine that has none. Sorted by id.
fn render_targets(store: &Store) -> Vec<(String, View)> {
    let mut out: Vec<(String, View)> = store
        .graph
        .views
        .iter()
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect();
    for m in store.graph.state_machines.values() {
        let id = format!("view:state/{}", entity_slug(&m.subject));
        if store.graph.views.contains_key(&id) {
            continue;
        }
        let Some(subject) = store.graph.entities.get(&m.subject) else {
            continue;
        };
        out.push((
            id,
            View {
                kind: "state".to_string(),
                title: subject.name.clone(),
                members: vec![m.subject.clone()],
                ..Default::default()
            },
        ));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn target(store: &Store, view_id: &str) -> Option<View> {
    render_targets(store)
        .into_iter()
        .find(|(id, _)| id == view_id)
        .map(|(_, v)| v)
}

// `<out>/diagrams/<kind>/<slug>` for a view id, without extension. None for an id that
// does not spell a kind and a slug.
fn diagram_base(out: &Path, view_id: &str) -> Option<PathBuf> {
    let rel = view_id.strip_prefix("view:")?;
    let (kind, slug) = rel.split_once('/')?;
    let clean = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    };
    if !clean(kind) || !clean(slug) {
        return None;
    }
    Some(out.join("diagrams").join(kind).join(slug))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderReport {
    pub rendered: Vec<String>,
    pub skipped: Vec<String>,
    // (view id, error)
    pub failed: Vec<(String, String)>,
    // `<kind>/<slug>` of renders whose view is gone.
    pub removed: Vec<String>,
}

fn remove_if_present(path: &Path) {
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}

// Emit and render every view into <out>/diagrams/, skipping the renderer where the
// .puml on disk already equals the emission and its .svg exists, removing the files of
// views that are gone. A render error keeps the .puml, drops the stale .svg, and is
// reported. Mirrors docs/compiler/diagrams.md#rendering and #output-layout.
pub fn render_all(store: &Store, out: &Path) -> RenderReport {
    let mut report = RenderReport::default();
    let mut keep: BTreeSet<PathBuf> = BTreeSet::new();
    for (id, view) in render_targets(store) {
        let Some(base) = diagram_base(out, &id) else {
            report
                .failed
                .push((id, "the id spells no kind and slug".to_string()));
            continue;
        };
        keep.insert(base.clone());
        let puml = emit(store, &id, &view);
        let puml_path = base.with_extension("puml");
        let svg_path = base.with_extension("svg");
        let png_path = base.with_extension("png");
        let unchanged = svg_path.exists()
            && std::fs::read_to_string(&puml_path)
                .ok()
                .is_some_and(|on_disk| on_disk == puml);
        if unchanged {
            report.skipped.push(id);
            continue;
        }
        if let Some(dir) = base.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                report.failed.push((id, e.to_string()));
                continue;
            }
        }
        if let Err(e) = std::fs::write(&puml_path, &puml) {
            report.failed.push((id, e.to_string()));
            continue;
        }
        remove_if_present(&png_path);
        match render_svg(&puml) {
            Ok(svg) => match std::fs::write(&svg_path, svg) {
                Ok(()) => report.rendered.push(id),
                Err(e) => report.failed.push((id, e.to_string())),
            },
            Err(e) => {
                remove_if_present(&svg_path);
                report.failed.push((id, e.0));
            }
        }
    }
    let dir = out.join("diagrams");
    let Ok(kinds) = std::fs::read_dir(&dir) else {
        return report;
    };
    for kind in kinds.flatten() {
        let kind_dir = kind.path();
        if !kind_dir.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&kind_dir) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !["puml", "svg", "png"].contains(&ext) {
                continue;
            }
            let base = path.with_extension("");
            if keep.contains(&base) {
                continue;
            }
            let _ = std::fs::remove_file(&path);
            let rel = format!(
                "{}/{}",
                kind.file_name().to_string_lossy(),
                base.file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default()
            );
            if !report.removed.contains(&rel) {
                report.removed.push(rel);
            }
        }
        if std::fs::read_dir(&kind_dir).is_ok_and(|mut d| d.next().is_none()) {
            let _ = std::fs::remove_dir(&kind_dir);
        }
    }
    report
}

// The PlantUML and SVG of one view, for surfaces that draw without touching the files.
pub fn render_view(store: &Store, view_id: &str) -> Result<(String, String), RenderError> {
    let view = target(store, view_id).ok_or_else(|| RenderError(format!("no view {}", view_id)))?;
    let puml = emit(store, view_id, &view);
    let svg = render_svg(&puml)?;
    Ok((puml, svg))
}

// The .png of a rendered view, rasterized on first request and kept beside the .svg.
pub fn png_for(out: &Path, view_id: &str) -> Result<PathBuf, RenderError> {
    let base =
        diagram_base(out, view_id).ok_or_else(|| RenderError(format!("no view {}", view_id)))?;
    let png_path = base.with_extension("png");
    if png_path.exists() {
        return Ok(png_path);
    }
    let svg = std::fs::read_to_string(base.with_extension("svg"))
        .map_err(|_| RenderError(format!("{} is not rendered", view_id)))?;
    let png = render_png(&svg)?;
    std::fs::write(&png_path, png).map_err(|e| RenderError(e.to_string()))?;
    Ok(png_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::derive::recompute;
    use crate::derive::tests::showcase_store;
    use crate::store::RecordBatch;
    use regex::Regex;

    // Mirrors docs/compiler/diagrams.md#level-views: a level's sequence view draws
    // each message endpoint lifted to its nearest ancestor among the level's
    // members, so a flow among leaves draws between their groupings; the frame
    // itself, when a flow starts there, draws as itself.
    #[test]
    fn a_level_sequence_view_lifts_endpoints_to_the_level_members() {
        let mut s = Store::default();
        let ent = |name: &str, parent: Option<&str>| Entity {
            name: name.into(),
            parent: parent.map(String::from),
            ..Default::default()
        };
        s.graph.entities.insert("ent:checkout".into(), ent("Checkout", None));
        s.graph.entities.insert("ent:cart".into(), ent("Cart", Some("ent:checkout")));
        s.graph.entities.insert("ent:funds".into(), ent("Funds", Some("ent:checkout")));
        s.graph
            .entities
            .insert("ent:gift-card".into(), ent("Gift Card", Some("ent:funds")));
        s.graph
            .entities
            .insert("ent:loyalty-point".into(), ent("Loyalty Point", Some("ent:funds")));
        let req = |text: &str, a: &str, b: &str| Requirement {
            statement: text.into(),
            entities: vec![a.into(), b.into()],
            edges: vec![ReqEdge {
                a: a.into(),
                b: b.into(),
                rel_type: Some("dependency".into()),
                cardinality: None,
            }],
            facets: vec![Facet {
                facet: "behavior".into(),
                reasoning: String::new(),
                measure: None,
            }],
            source: Some(SourceRef {
                doc: "checkout.md".into(),
                section: "/checkout".into(),
                quote: text.into(),
            }),
            ..Default::default()
        };
        s.graph.requirements.insert(
            "req:c-1".into(),
            req("A Gift Card pays for a Cart.", "ent:gift-card", "ent:cart"),
        );
        s.graph.requirements.insert(
            "req:c-2".into(),
            req("A Loyalty Point discounts a Cart.", "ent:loyalty-point", "ent:cart"),
        );
        s.graph.requirements.insert(
            "req:c-3".into(),
            req("The Checkout charges a Gift Card.", "ent:checkout", "ent:gift-card"),
        );
        // Internal to the funds: the funds level's flow, never a self-message above.
        s.graph.requirements.insert(
            "req:c-4".into(),
            req(
                "A Gift Card earns a Loyalty Point.",
                "ent:gift-card",
                "ent:loyalty-point",
            ),
        );
        let mut batch = RecordBatch::new(1);
        recompute(&mut s, "g1", &mut batch);
        let id = "view:sequence/checkout-funds-checkout";
        assert!(
            !s.graph.views[id].members.iter().any(|m| m == "req:c-4"),
            "{:?}",
            s.graph.views[id].members
        );
        assert!(
            s.graph.views["view:sequence/funds-gift-card-checkout"]
                .members
                .iter()
                .any(|m| m == "req:c-4"),
            "{:?}",
            s.graph.views.keys().collect::<Vec<_>>()
        );
        let view = s.graph.views.get(id).unwrap_or_else(|| {
            panic!("views: {:?}", s.graph.views.keys().collect::<Vec<_>>())
        });
        let puml = emit(&s, id, view);
        assert!(puml.contains("participant \"Funds\""), "{}", puml);
        assert!(puml.contains("participant \"Cart\""), "{}", puml);
        assert!(!puml.contains("participant \"Gift Card\""), "{}", puml);
        assert!(puml.contains("participant \"Checkout\""), "the frame draws as itself: {}", puml);
        // The drill-down participants agree with the drawing.
        let drawn = crate::derive::children_of_view(&s, id);
        assert!(drawn.iter().any(|(m, _)| m == "ent:funds"), "{:?}", drawn);
    }

    fn showcase() -> Store {
        let mut s = showcase_store();
        let mut batch = RecordBatch::new(1);
        recompute(&mut s, "g1", &mut batch);
        for (id, kind, members) in [
            (
                "view:usecase/checkout",
                "use-case",
                vec!["req:shop-1", "req:shop-3", "req:shop-7", "req:shop-8"],
            ),
            (
                "view:sequence/checkout",
                "sequence",
                vec!["req:shop-1", "req:shop-3"],
            ),
        ] {
            s.graph.views.insert(
                id.to_string(),
                View {
                    kind: kind.to_string(),
                    title: "Checkout".to_string(),
                    members: members.iter().map(|m| m.to_string()).collect(),
                    provenance: Some(Provenance::Decree {
                        author: "owner".into(),
                        at: "g1".into(),
                        note: None,
                    }),
                    ..Default::default()
                },
            );
        }
        s
    }

    fn view(kind: &str, title: &str, members: &[&str]) -> View {
        View {
            kind: kind.to_string(),
            title: title.to_string(),
            members: members.iter().map(|m| m.to_string()).collect(),
            ..Default::default()
        }
    }

    fn emit_kind(s: &Store, kind: &str, members: &[&str]) -> String {
        let v = view(kind, "Checkout", members);
        emit(s, &format!("view:{}/checkout", view_kind_slug(kind)), &v)
    }

    // Structural lines: aliases replaced by display names, quotes dropped, whitespace
    // collapsed, free-text labels reduced to their requirement id or step number, the
    // drill-down links dropped (their own tests read the raw text).
    fn canon(puml: &str) -> Vec<String> {
        let decl = Regex::new(r#"^(\w+) (?:"([^"]*)"|([^\s"]+)) as ([^\s{\[]+)(.*)$"#).unwrap();
        let part = Regex::new(r"^\[([^\]]+)\] as (\S+)(.*)$").unwrap();
        let req = Regex::new(r"\((req:[^)]+)\)").unwrap();
        let num = Regex::new(r"^(\d+\.)").unwrap();
        let link = Regex::new(r" \[\[[^\]]*\]\]").unwrap();
        let mut map: Vec<(String, String)> = Vec::new();
        let mut out = Vec::new();
        for raw in puml.lines() {
            let line = link.replace_all(&oneline(raw), "").to_string();
            if line.is_empty() {
                continue;
            }
            // A declaration names its element directly; only other lines carry aliases.
            if let Some(c) = decl.captures(&line) {
                let name = c.get(2).or(c.get(3)).map(|m| m.as_str()).unwrap();
                map.push((c[4].to_string(), name.to_string()));
                out.push(format!("{} {}{}", &c[1], name, &c[5]).replace('"', ""));
                continue;
            }
            if let Some(c) = part.captures(&line) {
                map.push((c[2].to_string(), c[1].to_string()));
                out.push(format!("[{}]{}", &c[1], &c[3]));
                continue;
            }
            let (head, label) = match line.split_once(" : ") {
                Some((h, l)) => (h.to_string(), Some(l.to_string())),
                None => (line.clone(), None),
            };
            let head = head
                .split(' ')
                .map(|tok| {
                    let t = tok.trim_matches('"');
                    map.iter()
                        .find(|(a, _)| a == t)
                        .map(|(_, n)| n.as_str())
                        .unwrap_or(t)
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join(" ");
            let label = label.map(|l| {
                if let Some(c) = num.captures(&l) {
                    c[1].to_string()
                } else if let Some(c) = req.captures(&l) {
                    format!("({})", &c[1])
                } else {
                    l
                }
            });
            let line = match label {
                Some(l) => format!("{} : {}", head, l),
                None => head,
            };
            let line = if line.starts_with(':') && line.ends_with(';') {
                match req.captures(&line) {
                    Some(c) => format!(":({});", &c[1]),
                    None => line,
                }
            } else {
                line
            };
            out.push(line.replace('"', ""));
        }
        out
    }

    fn lines_with(puml: &str, needle: &str) -> Vec<String> {
        canon(puml)
            .into_iter()
            .filter(|l| l.contains(needle))
            .collect()
    }

    #[test]
    fn class_emitter_matches_the_showcase() {
        let s = showcase();
        let puml = emit_kind(
            &s,
            "class",
            &[
                "ent:customer",
                "ent:shopping-cart",
                "ent:order",
                "ent:order-item",
            ],
        );
        // The showcase block, plus the cart's two attributes the showcase graph states
        // and the block leaves out by hand.
        let expected = "@startuml\nclass Customer <<actor>> {\n  tier : string\n}\nclass \"Shopping Cart\" as Cart {\n  items\n  currency\n}\nclass Order {\n  total\n  currency\n}\nclass \"Order Item\" as Item\nCustomer -- Cart\nCart *-- \"1..*\" Item\n@enduml\n";
        assert_eq!(canon(&puml), canon(expected), "{}", puml);
        assert!(!puml.contains("title "));
    }

    #[test]
    fn object_emitter_names_types_and_values() {
        let s = showcase();
        let puml = emit_kind(&s, "object", &["ent:ana", "ent:anas-cart"]);
        let c = canon(&puml);
        assert!(
            c.contains(&"object Ana : Customer {".to_string()),
            "{}",
            puml
        );
        assert!(
            c.contains(&"object Ana's cart : Shopping Cart {".to_string()),
            "{}",
            puml
        );
        assert!(c.contains(&"tier = gold".to_string()));
        assert!(c.contains(&"items = 3".to_string()));
        assert!(c.contains(&"currency = EUR".to_string()));
        assert!(
            c.contains(&"Ana : Customer -- Ana's cart : Shopping Cart".to_string()),
            "{:?}",
            c
        );
        // No instantiation arrow.
        assert_eq!(
            c.iter()
                .filter(|l| l.contains(" -- ") || l.contains("..>"))
                .count(),
            1
        );
    }

    #[test]
    fn package_emitter_nests_classes_and_never_emits_an_empty_body() {
        let s = showcase();
        let puml = emit_kind(
            &s,
            "package",
            &[
                "ent:order-service",
                "ent:shopping-cart",
                "ent:order",
                "ent:order-item",
                "ent:inventory-service",
            ],
        );
        // The showcase block, minus the empty body it draws for the inventory service.
        let expected = "@startuml\npackage \"Order Service\" as OS {\n  class \"Shopping Cart\"\n  class Order\n  class \"Order Item\"\n}\npackage \"Inventory Service\" as IS\nOS ..> IS\n@enduml\n";
        assert_eq!(canon(&puml), canon(expected), "{}", puml);
        assert!(!puml.contains("{\n}"), "{}", puml);
        // Two leaf packages: one-line forms and the lifted arrow only.
        let puml = emit_kind(
            &s,
            "package",
            &["ent:order-service", "ent:inventory-service"],
        );
        assert!(!puml.contains('{'), "{}", puml);
        assert_eq!(
            canon(&puml),
            vec![
                "@startuml",
                "package Order Service",
                "package Inventory Service",
                "Order Service ..> Inventory Service",
                "@enduml"
            ]
        );
    }

    #[test]
    fn component_emitter_draws_lollipops_and_sockets() {
        let s = showcase();
        let puml = emit_kind(
            &s,
            "component",
            &[
                "ent:customer",
                "ent:order-service",
                "ent:inventory-service",
                "ent:checkout-api",
                "ent:stock-api",
            ],
        );
        // The showcase block, plus the customer's association with the shopping cart
        // lifted to the order service that hides it.
        let expected = "@startuml\nactor Customer\ncomponent \"Order Service\" as OS <<service>>\ncomponent \"Inventory Service\" as IS <<service>>\ninterface \"checkout API\" as C\ninterface \"stock API\" as S\nCustomer ..> C\nOS -- C\nCustomer -- OS\nIS -- S\nOS --( S : use\n@enduml\n";
        let mut mine = canon(&puml);
        let mut want = canon(expected);
        mine.sort();
        want.sort();
        assert_eq!(mine, want, "{}", puml);
        // Arrows come in relationship id order: the pair slugs, lexical.
        let arrows: Vec<String> = canon(&puml)
            .into_iter()
            .filter(|l| l.contains(" -- ") || l.contains("..>") || l.contains("--("))
            .collect();
        assert_eq!(
            arrows,
            vec![
                "Customer ..> checkout API",
                "Order Service -- checkout API",
                "Customer -- Order Service",
                "Inventory Service -- stock API",
                "Order Service --( stock API : use"
            ]
        );
        // The shop's derived level view is the docs' example: the two services, the
        // customer whose edges lift into the level, and the drill-down link on the
        // service that holds a level of its own (its four children); the customer's
        // association with the hidden cart joins its lifted dependency under one arrow.
        let puml = emit(
            &s,
            "view:component/shop",
            &s.graph.views["view:component/shop"],
        );
        assert_eq!(
            canon(&puml),
            vec![
                "@startuml",
                "component Inventory Service <<service>>",
                "component Order Service <<service>>",
                "actor Customer",
                "Customer -- Order Service : 2 edges",
                "Order Service ..> Inventory Service",
                "@enduml"
            ],
            "{}",
            puml
        );
        assert!(
            puml.contains(
                "component \"Order Service\" as order_service <<service>> [[../component/order-service.svg]]\n"
            ),
            "{}",
            puml
        );
        assert!(
            puml.contains("component \"Inventory Service\" as inventory_service <<service>>\n"),
            "{}",
            puml
        );
    }

    #[test]
    fn composite_emitter_frames_parts() {
        let s = showcase();
        let puml = emit_kind(
            &s,
            "composite",
            &[
                "ent:order-service",
                "ent:shopping-cart",
                "ent:order",
                "ent:order-item",
            ],
        );
        let c = canon(&puml);
        assert_eq!(c[1], "component Order Service {");
        assert_eq!(c[2], "[Shopping Cart]");
        assert_eq!(c[3], "[Order]");
        assert_eq!(c[4], "[Order Item]");
        assert_eq!(c[5], "}");
        assert_eq!(c[6], "Shopping Cart *-- 1..* Order Item");
    }

    #[test]
    fn deployment_emitter_places_artifacts_by_value() {
        let s = showcase();
        let puml = emit_kind(&s, "deployment", &["ent:shop", "ent:customer"]);
        let c = canon(&puml);
        assert_eq!(c[1], "node EU region {");
        assert_eq!(c[2], "artifact Shop");
        assert_eq!(c[3], "}");
        assert_eq!(c[4], "artifact Customer");
    }

    #[test]
    fn use_case_emitter_links_actors() {
        let s = showcase();
        let puml = emit(
            &s,
            "view:usecase/checkout",
            &s.graph.views["view:usecase/checkout"],
        );
        let expected =
            "@startuml\nactor Customer\nusecase Checkout\nCustomer -- Checkout\n@enduml\n";
        assert_eq!(canon(&puml), canon(expected), "{}", puml);
    }

    #[test]
    fn activity_emitter_branches_on_shared_transitions() {
        let s = showcase();
        let puml = emit_kind(
            &s,
            "activity",
            &["req:shop-1", "req:shop-3", "req:shop-7", "req:shop-8"],
        );
        assert_eq!(
            canon(&puml),
            vec![
                "@startuml",
                "start",
                ":(req:shop-1);",
                ":(req:shop-3);",
                "if (payment succeeds?) then (yes)",
                ":(req:shop-7);",
                "else (no)",
                ":(req:shop-8);",
                "endif",
                "stop",
                "@enduml"
            ],
            "{}",
            puml
        );
        assert!(puml.contains(":When payment succeeds, the order becomes paid. (req:shop-7);"));
    }

    #[test]
    fn state_emitter_matches_the_showcase() {
        let s = showcase();
        let puml = emit(&s, "view:state/order", &s.graph.views["view:state/order"]);
        assert_eq!(
            puml,
            "@startuml\n[*] --> placed\nplaced --> paid : payment succeeds\nplaced --> held : payment declined\n@enduml\n"
        );
        // A subject without a machine draws as one state; spaced names get aliases.
        let puml = emit_kind(&s, "state", &["ent:customer"]);
        assert!(puml.contains("state \"Customer\" as customer"));
        let mut s = s;
        s.graph
            .requirements
            .get_mut("req:shop-8")
            .unwrap()
            .transition
            .as_mut()
            .unwrap()
            .to = "held for review".into();
        recompute(&mut s, "g2", &mut RecordBatch::new(2));
        let puml = emit(&s, "view:state/order", &s.graph.views["view:state/order"]);
        assert!(
            puml.contains("state \"held for review\" as held_for_review"),
            "{}",
            puml
        );
        assert!(puml.contains("placed --> held_for_review : payment declined"));
    }

    #[test]
    fn sequence_emitter_resolves_interfaces_to_providers() {
        let s = showcase();
        let puml = emit(
            &s,
            "view:sequence/checkout",
            &s.graph.views["view:sequence/checkout"],
        );
        let expected = "@startuml\nactor Customer\nCustomer -> \"Order Service\" : submit cart (req:shop-1)\n\"Order Service\" -> \"Inventory Service\" : reserve stock (req:shop-3)\n@enduml\n";
        assert_eq!(
            lines_with(&puml, "->"),
            lines_with(expected, "->"),
            "{}",
            puml
        );
        assert_eq!(lines_with(&puml, "actor "), vec!["actor Customer"]);
        assert!(puml.contains("participant \"Order Service\" as order_service"));
        // A member with no edge is a self-message on its first entity.
        let puml = emit_kind(&s, "sequence", &["req:shop-1", "req:shop-5"]);
        assert!(
            lines_with(&puml, "->").contains(&"Order -> Order : (req:shop-5)".to_string()),
            "{}",
            puml
        );
    }

    #[test]
    fn communication_emitter_numbers_messages() {
        let s = showcase();
        let puml = emit_kind(&s, "communication", &["req:shop-1", "req:shop-3"]);
        let expected = "@startuml\nactor Customer\nrectangle \"Order Service\" as OS\nrectangle \"Inventory Service\" as IS\nCustomer -> OS : 1. submit cart\nOS -> IS : 2. reserve stock\n@enduml\n";
        assert_eq!(canon(&puml), canon(expected), "{}", puml);
    }

    #[test]
    fn timing_emitter_marks_the_measure() {
        let s = showcase();
        let puml = emit_kind(&s, "timing", &["ent:order", "req:shop-10"]);
        let expected = "@startuml\ntitle checkout confirms within 2s (req:shop-10)\nrobust \"Order\" as O\n@0\nO is placed\n@2\nO is paid\n@enduml\n";
        let c = canon(&puml);
        let want = canon(expected);
        assert_eq!(c[2..], want[2..], "{}", puml);
        assert!(
            c[1].starts_with(
                "title The shop shall confirm checkout within 2 seconds. (req:shop-10)"
            ),
            "{}",
            puml
        );
        // No measure: the lane and nothing else.
        let puml = emit_kind(&s, "timing", &["ent:order"]);
        assert_eq!(canon(&puml), vec!["@startuml", "robust Order", "@enduml"]);
    }

    #[test]
    fn overview_emitter_references_sequence_views() {
        let s = showcase();
        let puml = emit_kind(
            &s,
            "overview",
            &["req:shop-1", "req:shop-3", "req:shop-7", "req:shop-8"],
        );
        assert_eq!(
            canon(&puml),
            vec![
                "@startuml",
                "start",
                ":ref: view:sequence/checkout;",
                ":(req:shop-7);",
                ":(req:shop-8);",
                "stop",
                "@enduml"
            ],
            "{}",
            puml
        );
    }

    fn tree_store() -> Store {
        let mut s = Store::default();
        for (id, parent) in [
            ("ent:a", None),
            ("ent:a1", Some("ent:a")),
            ("ent:a2", Some("ent:a")),
            ("ent:b", None),
            ("ent:b1", Some("ent:b")),
        ] {
            s.graph.entities.insert(
                id.into(),
                Entity {
                    name: id.trim_start_matches("ent:").to_uppercase(),
                    parent: parent.map(String::from),
                    ..Default::default()
                },
            );
        }
        s
    }

    fn add_req(s: &mut Store, id: &str, a: &str, b: &str, t: &str) {
        s.graph.requirements.insert(
            id.into(),
            Requirement {
                statement: format!("{} uses {}", a, b),
                entities: vec![a.into(), b.into()],
                edges: vec![ReqEdge {
                    a: a.into(),
                    b: b.into(),
                    rel_type: Some(t.into()),
                    cardinality: None,
                }],
                source: Some(SourceRef {
                    doc: "t.md".into(),
                    section: "/t".into(),
                    quote: "q".into(),
                }),
                ..Default::default()
            },
        );
    }

    #[test]
    fn lifting_and_collapse_aggregate_hidden_edges() {
        let mut s = tree_store();
        add_req(&mut s, "req:t-1", "ent:a1", "ent:b1", "dependency");
        recompute(&mut s, "g1", &mut RecordBatch::new(1));
        let system = view("class", "System", &["ent:a", "ent:b"]);
        let puml = emit(&s, "view:class/system", &system);
        assert_eq!(
            canon(&puml),
            vec!["@startuml", "class A", "class B", "A ..> B", "@enduml"],
            "{}",
            puml
        );
        // A second type on the same lifted pair: one arrow, the stronger type, a count.
        add_req(&mut s, "req:t-2", "ent:a1", "ent:b1", "association");
        // An instantiation never joins; an edge inside one subtree is not drawn.
        add_req(&mut s, "req:t-3", "ent:a1", "ent:b1", "instantiation");
        add_req(&mut s, "req:t-4", "ent:a1", "ent:a2", "composition");
        recompute(&mut s, "g2", &mut RecordBatch::new(2));
        let puml = emit(&s, "view:class/system", &system);
        assert_eq!(
            canon(&puml),
            vec![
                "@startuml",
                "class A",
                "class B",
                "A -- B : 2 edges",
                "@enduml"
            ],
            "{}",
            puml
        );
        // Listing the leaves draws them and their own arrows; collapsing a hides them
        // again and links the sub-view that details a.
        let leaves = view(
            "class",
            "Leaves",
            &["ent:a", "ent:a1", "ent:a2", "ent:b", "ent:b1"],
        );
        let puml = emit(&s, "view:class/leaves", &leaves);
        let c = canon(&puml);
        assert!(c.contains(&"A1 -- B1 : 2 edges".to_string()), "{}", puml);
        assert!(c.contains(&"A1 *-- A2".to_string()), "{}", puml);
        s.graph.views.insert(
            "view:class/a-parts".into(),
            View {
                query: Some(ViewQuery {
                    parent: Some("ent:a".into()),
                    ..Default::default()
                }),
                ..view("class", "A parts", &[])
            },
        );
        let collapsed = View {
            collapse: vec!["ent:a".into()],
            ..leaves
        };
        let puml = emit(&s, "view:class/leaves", &collapsed);
        assert!(
            puml.contains("class \"A\" as a [[../class/a-parts.svg]]"),
            "{}",
            puml
        );
        assert!(!puml.contains("A1"), "{}", puml);
        assert!(
            canon(&puml).contains(&"A -- B1 : 2 edges".to_string()),
            "{}",
            puml
        );
    }

    // ent:a holds two children and ent:b one: a has a level view, b is a leaf for
    // drill-down.
    fn level_tree() -> Store {
        let mut s = tree_store();
        add_req(&mut s, "req:t-1", "ent:a1", "ent:b1", "dependency");
        recompute(&mut s, "g1", &mut RecordBatch::new(1));
        s
    }

    #[test]
    fn structural_emitters_link_members_with_level_views() {
        let s = level_tree();
        let a_view = crate::derive::level_view_id(&s, "ent:a").expect("two children make a level");
        assert!(crate::derive::level_view_id(&s, "ent:b").is_none());
        let link = view_link(&a_view);
        for kind in ["class", "component"] {
            let v = view(kind, "System", &["ent:a", "ent:b"]);
            let puml = emit(&s, &format!("view:{}/system", kind), &v);
            assert!(
                puml.contains(&format!("{} \"A\" as a {}", kind, link)),
                "{}",
                puml
            );
            assert!(puml.contains(&format!("{} \"B\" as b\n", kind)), "{}", puml);
            assert_eq!(puml.matches("[[").count(), 1, "{}", puml);
        }
        // A view never links to itself, and a leaf carries nothing.
        let own = view("class", "A", &["ent:a", "ent:a1", "ent:a2"]);
        assert!(!emit(&s, &a_view, &own).contains("[["));
    }

    #[test]
    fn use_case_actors_link_to_their_level() {
        let mut s = tree_store();
        for id in ["ent:a", "ent:b"] {
            s.graph.entities.get_mut(id).unwrap().stereotype = Some("actor".into());
        }
        add_req(&mut s, "req:t-1", "ent:a", "ent:b1", "dependency");
        add_req(&mut s, "req:t-2", "ent:b", "ent:a1", "dependency");
        recompute(&mut s, "g1", &mut RecordBatch::new(1));
        let link = view_link(&crate::derive::level_view_id(&s, "ent:a").unwrap());
        let v = view("use-case", "Flow", &["req:t-1", "req:t-2"]);
        let puml = emit(&s, "view:usecase/flow", &v);
        assert!(
            puml.contains(&format!("actor \"A\" as a {}\n", link)),
            "{}",
            puml
        );
        assert!(puml.contains("actor \"B\" as b\n"), "{}", puml);
        // The in-process crate would split a linked actor in two: one actor per member.
        let svg = in_process_svg(&puml).unwrap();
        assert_eq!(svg.matches("class=\"entity\"").count(), 3, "{}", svg);
        assert!(svg.contains("xlink:href=\"../"), "{}", svg);
        assert_eq!(svg.matches("<a ").count(), 1, "{}", svg);
    }

    #[test]
    fn in_process_renderings_carry_anchors_for_links() {
        let s = level_tree();
        let a_view = crate::derive::level_view_id(&s, "ent:a").unwrap();
        let href = format!("../{}.svg", &a_view["view:".len()..]);
        for kind in ["class", "component"] {
            let v = view(kind, "System", &["ent:a", "ent:b"]);
            let puml = emit(&s, &format!("view:{}/system", kind), &v);
            assert!(puml.contains("[["), "{}", puml);
            let svg = render_svg(&puml).unwrap();
            let anchor = format!(
                "<a href=\"{0}\" xlink:href=\"{0}\"><g class=\"entity\" data-qualified-name=\"a\"",
                href
            );
            assert_eq!(svg.matches(&anchor).count(), 1, "{}", svg);
            assert_eq!(svg.matches("<a ").count(), 1, "{}", svg);
            assert_eq!(svg.matches("</a>").count(), 1, "{}", svg);
            // The label survives the link: the crate alone would draw the alias.
            assert!(svg.contains(">A</text>"), "{}", svg);
            assert!(!svg.contains(">a</text>"), "{}", svg);
            assert!(!svg.contains("[["), "{}", svg);
            // The anchored svg still rasterizes.
            render_png(&svg).unwrap();
        }
    }

    #[test]
    fn element_links_strip_by_alias_or_name_and_anchor_by_key() {
        let puml = "@startuml\nclass \"A b\" as a <<svc>> [[../class/a.svg]] {\n  x : int\n}\npackage \"P q\" [[../class/p.svg]]\nA -- B : see [[not a link]]\n@enduml\n";
        let (plain, links) = strip_element_links(puml);
        assert_eq!(
            plain,
            "@startuml\nclass \"A b\" as a <<svc>> {\n  x : int\n}\npackage \"P q\"\nA -- B : see [[not a link]]\n@enduml\n"
        );
        assert_eq!(
            links,
            vec![
                ("a".to_string(), "../class/a.svg".to_string()),
                ("P q".to_string(), "../class/p.svg".to_string())
            ]
        );
        // A key the crate drew nothing for keeps no anchor; nested groups close where
        // the entity's own group does.
        let svg = "<svg><g><g class=\"entity\" data-qualified-name=\"a\" id=\"e1\"><g><text>A b</text></g></g><g class=\"entity\" data-qualified-name=\"b\"><text>B</text></g></g></svg>";
        assert_eq!(
            anchor_entities(svg, &links),
            "<svg><g><a href=\"../class/a.svg\" xlink:href=\"../class/a.svg\"><g class=\"entity\" data-qualified-name=\"a\" id=\"e1\"><g><text>A b</text></g></g></a><g class=\"entity\" data-qualified-name=\"b\"><text>B</text></g></g></svg>"
        );
        assert_eq!(anchor_entities(svg, &[]), svg);
    }

    #[test]
    fn over_limit_views_auto_collapse_or_mark_every_member() {
        // Twenty-one roots over a bump of ten (hard twenty): nothing to collapse.
        let mut s = Store::default();
        let ids: Vec<String> = (0..21).map(|i| format!("ent:root-{}", i)).collect();
        for id in &ids {
            s.graph.entities.insert(
                id.clone(),
                Entity {
                    name: id.clone(),
                    ..Default::default()
                },
            );
        }
        let members: Vec<&str> = ids.iter().map(String::as_str).collect();
        let mut v = view("class", "Roots", &members);
        v.limits.insert(
            "members-per-structural-view".into(),
            LimitBump { value: 10 },
        );
        let puml = emit(&s, "view:class/roots", &v);
        assert!(
            puml.contains("title Roots (over limit: 21 members)"),
            "{}",
            puml
        );
        assert_eq!(puml.matches("\nclass ").count(), 21);
        // Without the bump, twenty-one is over soft only: no intervention.
        let plain = view("class", "Roots", &members);
        assert!(!emit(&s, "view:class/roots", &plain).contains("title "));

        // A root with twenty-one listed children: the largest subtree collapses.
        let mut s = Store::default();
        s.graph.entities.insert(
            "ent:r".into(),
            Entity {
                name: "R".into(),
                ..Default::default()
            },
        );
        let kids: Vec<String> = (0..21).map(|i| format!("ent:k-{}", i)).collect();
        for id in &kids {
            s.graph.entities.insert(
                id.clone(),
                Entity {
                    name: id.clone(),
                    parent: Some("ent:r".into()),
                    ..Default::default()
                },
            );
        }
        let mut members = vec!["ent:r".to_string()];
        members.extend(kids.iter().cloned());
        let members: Vec<&str> = members.iter().map(String::as_str).collect();
        let mut v = view("class", "Tree", &members);
        v.limits.insert(
            "members-per-structural-view".into(),
            LimitBump { value: 10 },
        );
        let puml = emit(&s, "view:class/tree", &v);
        assert!(
            puml.contains("title Tree (collapsed: 1 subtrees over limit)"),
            "{}",
            puml
        );
        assert_eq!(puml.matches("\nclass ").count(), 1);
        assert!(puml.contains("class \"R\" as r"));

        // A flow view past the hard member count (twenty) renders every member, marked.
        let mut s = Store::default();
        s.graph.entities.insert(
            "ent:x".into(),
            Entity {
                name: "X".into(),
                ..Default::default()
            },
        );
        let steps: Vec<String> = (0..21).map(|i| format!("req:f-{}", i)).collect();
        for (i, id) in steps.iter().enumerate() {
            s.graph.requirements.insert(
                id.clone(),
                Requirement {
                    statement: format!("Step {}.", i),
                    entities: vec!["ent:x".into()],
                    ..Default::default()
                },
            );
        }
        let members: Vec<&str> = steps.iter().map(String::as_str).collect();
        let v = view("activity", "Flow", &members);
        let puml = emit(&s, "view:activity/flow", &v);
        assert!(
            puml.contains("title Flow (over limit: 21 members)"),
            "{}",
            puml
        );
        assert_eq!(puml.matches("(req:f-").count(), 21);
        // Twenty is within the hard threshold: no mark.
        let v = view("activity", "Flow", &members[..20]);
        assert!(!emit(&s, "view:activity/flow", &v).contains("title "));
    }

    #[test]
    fn every_emitter_renders_through_the_crate() {
        let s = showcase();
        let cases: Vec<(&str, Vec<&str>)> = vec![
            (
                "class",
                vec![
                    "ent:customer",
                    "ent:shopping-cart",
                    "ent:order",
                    "ent:order-item",
                ],
            ),
            ("object", vec!["ent:ana", "ent:anas-cart"]),
            (
                "package",
                vec![
                    "ent:order-service",
                    "ent:shopping-cart",
                    "ent:order",
                    "ent:order-item",
                    "ent:inventory-service",
                ],
            ),
            (
                "component",
                vec![
                    "ent:customer",
                    "ent:order-service",
                    "ent:inventory-service",
                    "ent:checkout-api",
                    "ent:stock-api",
                ],
            ),
            (
                "composite",
                vec![
                    "ent:order-service",
                    "ent:shopping-cart",
                    "ent:order",
                    "ent:order-item",
                ],
            ),
            ("deployment", vec!["ent:shop", "ent:customer"]),
            (
                "use-case",
                vec!["req:shop-1", "req:shop-3", "req:shop-7", "req:shop-8"],
            ),
            (
                "activity",
                vec!["req:shop-1", "req:shop-3", "req:shop-7", "req:shop-8"],
            ),
            ("state", vec!["ent:order"]),
            ("sequence", vec!["req:shop-1", "req:shop-3"]),
            ("communication", vec!["req:shop-1", "req:shop-3"]),
            ("timing", vec!["ent:order", "req:shop-10"]),
            (
                "overview",
                vec!["req:shop-1", "req:shop-3", "req:shop-7", "req:shop-8"],
            ),
        ];
        assert_eq!(cases.len(), VIEW_KINDS.len());
        for (kind, members) in &cases {
            let puml = emit_kind(&s, kind, members);
            let svg = render_svg(&puml).unwrap_or_else(|e| panic!("{}: {}\n{}", kind, e, puml));
            assert!(svg.contains("<svg"), "{}", kind);
            // An empty view of the kind renders too.
            let puml = emit_kind(&s, kind, &[]);
            render_svg(&puml).unwrap_or_else(|e| panic!("empty {}: {}\n{}", kind, e, puml));
        }
        // The collapsed node's link and the over-limit title render.
        let mut v = view(
            "class",
            "Public",
            &["ent:shop", "ent:order-service", "ent:order"],
        );
        v.collapse.push("ent:shop".into());
        let puml = emit(&s, "view:class/public", &v);
        render_svg(&puml).unwrap_or_else(|e| panic!("{}\n{}", e, puml));
    }

    // The scope root's level view: view:class/public or view:component/public by the
    // kind rule, whichever the derivation minted.
    fn root_view(s: &Store) -> String {
        ["view:component/public", "view:class/public"]
            .into_iter()
            .find(|id| s.graph.views.contains_key(*id))
            .map(String::from)
            .expect("the showcase derives a root level view")
    }

    #[test]
    fn render_svg_draws_the_showcase_root_view() {
        let s = showcase();
        let root = root_view(&s);
        let (puml, svg) = render_view(&s, &root).unwrap();
        assert!(puml.starts_with("@startuml\n"));
        // Every top-level member is drawn by its element group, except the actor: the
        // in-process crate draws no actor in a component diagram (a renderer gap the
        // official binary does not share, so the emitter keeps declaring it). The
        // shop, which holds a level, is the one anchor; the customer beside it is a
        // leaf and would carry none.
        let members = &s.graph.views[&root].members;
        assert!(members.contains(&"ent:shop".to_string()), "{:?}", members);
        assert!(
            members.contains(&"ent:customer".to_string()),
            "{:?}",
            members
        );
        assert!(
            puml.contains("actor \"Customer\" as customer\n"),
            "{}",
            puml
        );
        for m in members.iter().filter(|m| **m != "ent:customer") {
            let group = format!("data-qualified-name=\"{}\"", alias_of(m));
            assert!(svg.contains(&group), "{} missing from\n{}", m, svg);
        }
        assert!(svg.contains(">Shop</text>"), "{}", svg);
        assert!(
            svg.contains(
                "<a href=\"../component/shop.svg\" xlink:href=\"../component/shop.svg\"><g class=\"entity\" data-qualified-name=\"shop\""
            ),
            "{}",
            svg
        );
        assert_eq!(svg.matches("<a ").count(), 1, "{}", svg);
        assert!(render_view(&s, "view:class/nope").is_err());
        assert!(render_svg("@startuml\nthis is not plantuml at all ((\n@enduml\n").is_err());
        // A defect inside the crate (it measures edge labels byte by byte, so a
        // multi-byte character in one panics) surfaces as an error, never an abort.
        if let Err(e) = render_svg("@startuml\nclass A\nclass B\nA -- B : ×2\n@enduml\n") {
            assert!(e.0.contains("renderer panicked"), "{}", e);
        }
    }

    #[cfg(unix)]
    #[test]
    fn native_binary_swap_pipes_stdin_and_reports_a_failed_exit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp("native");
        let stub = dir.join("plantuml");
        std::fs::write(
            &stub,
            "#!/bin/sh\ntest \"$1\" = -tsvg && test \"$2\" = -pipe && cat\n",
        )
        .unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let puml = "@startuml\nclass A\n@enduml\n";
        assert_eq!(native_svg(stub.to_str().unwrap(), puml).unwrap(), puml);
        let err = native_svg("/usr/bin/false", puml).unwrap_err();
        assert!(err.0.contains("exited with"), "{}", err);
        assert!(native_svg(dir.join("missing").to_str().unwrap(), puml).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_png_yields_a_png_header() {
        let s = showcase();
        let (_, svg) = render_view(&s, "view:state/order").unwrap();
        let png = render_png(&svg).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }

    fn tmp(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("jazyk-render-{}-{}", label, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn render_all_writes_paths_and_skips_unchanged() {
        let out = tmp("all");
        let mut s = showcase();
        let report = render_all(&s, &out);
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert!(report.skipped.is_empty());
        // Every target renders; the curated views and the defaults whose rule is fixed
        // by the showcase are named, the level and flow defaults come from derivation.
        let expected: Vec<String> = render_targets(&s)
            .into_iter()
            .map(|(id, _)| id.strip_prefix("view:").unwrap().to_string())
            .collect();
        for rel in [
            "component/shop",
            "state/order",
            "object/customer",
            "object/shopping-cart",
            "usecase/checkout",
            "sequence/checkout",
        ] {
            assert!(expected.contains(&rel.to_string()), "{:?}", expected);
        }
        assert!(expected.contains(&root_view(&s)[5..].to_string()));
        for rel in &expected {
            assert!(
                out.join("diagrams").join(format!("{}.puml", rel)).exists(),
                "{}",
                rel
            );
            assert!(
                out.join("diagrams").join(format!("{}.svg", rel)).exists(),
                "{}",
                rel
            );
        }
        assert_eq!(report.rendered.len(), expected.len());
        // A second run renders nothing.
        let again = render_all(&s, &out);
        assert!(again.rendered.is_empty(), "{:?}", again.rendered);
        assert_eq!(again.skipped.len(), expected.len());
        assert!(again.removed.is_empty());
        // A png on request, dropped when the puml changes.
        let png = png_for(&out, "view:state/order").unwrap();
        assert!(png.exists());
        assert_eq!(png_for(&out, "view:state/order").unwrap(), png);
        assert!(png_for(&out, "view:state/nope").is_err());
        // A deleted curated view loses its files; a machine without a state view still
        // renders as view:state/<slug>; a changed view re-renders and loses its png.
        s.graph.views.remove("view:sequence/checkout");
        s.graph.views.remove("view:state/order");
        s.graph
            .views
            .get_mut("view:usecase/checkout")
            .unwrap()
            .title = "Checkout flow".into();
        let third = render_all(&s, &out);
        assert_eq!(third.removed, vec!["sequence/checkout".to_string()]);
        assert!(!out.join("diagrams/sequence/checkout.svg").exists());
        assert!(out.join("diagrams/state/order.svg").exists());
        assert!(third
            .rendered
            .contains(&"view:usecase/checkout".to_string()));
        assert!(third.skipped.contains(&"view:state/order".to_string()));
        assert!(png.exists());
        s.graph
            .requirements
            .get_mut("req:shop-7")
            .unwrap()
            .transition
            .as_mut()
            .unwrap()
            .trigger = Some("payment cleared".into());
        recompute(&mut s, "g3", &mut RecordBatch::new(3));
        let fourth = render_all(&s, &out);
        assert!(fourth.rendered.contains(&"view:state/order".to_string()));
        assert!(!png.exists());
        let _ = std::fs::remove_dir_all(&out);
    }
}
