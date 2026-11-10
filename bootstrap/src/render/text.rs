use super::scene::{Arrow, Scene};
use crate::ids::entity_slug;
use crate::model::*;

// ---- text helpers ----

pub(super) fn oneline(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn quoted(name: &str) -> String {
    format!("\"{}\"", oneline(name).replace('"', "'"))
}

// The `[Part]` form of a component view: a bracket inside the name would end it, so
// it renders as a parenthesis. Mirrors docs/compiler/diagrams.md#the-emitters.
pub(super) fn bracketed(name: &str) -> String {
    format!("[{}]", oneline(name).replace('[', "(").replace(']', ")"))
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

pub(super) fn identifier(base: &str, prefix: &str) -> String {
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

// The PlantUML alias of an entity: derived from its id, stable across builds, and
// distinct for distinct ids. A plain slug (letters and digits joined by single
// hyphens, not opening with a digit or spelling a keyword) maps hyphens to
// underscores, and two plain slugs never meet. Any other id (`a_b`, `a--b`, `class`)
// keeps its readable form and takes `__` and a hash of the id, a pair a plain slug
// cannot spell, so `ent:a-b` and `ent:a_b` stay two elements.
// Mirrors docs/compiler/diagrams.md#the-emitters.
pub(super) fn alias_of(id: &str) -> String {
    let slug = entity_slug(id);
    let plain = !slug.is_empty()
        && slug
            .split('-')
            .all(|w| !w.is_empty() && w.chars().all(char::is_alphanumeric))
        && !slug.starts_with(|c: char| c.is_ascii_digit())
        && !PLANTUML_WORDS.contains(&slug.replace('-', "_").as_str());
    if plain {
        slug.replace('-', "_")
    } else {
        format!(
            "{}__{}",
            identifier(slug, "n_"),
            &crate::model::hash_hex(id)[..8]
        )
    }
}

// A state name usable as a PlantUML token, or the alias it needs a declaration for.
pub(super) fn state_token(name: &str) -> (Option<String>, String) {
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

// Graph text is inert. A line that would open a preprocessor directive (`!include`),
// end the diagram (`@enduml`), or call a builtin (`%getenv`), and inline image markup
// (`<img:...>`) anywhere, gets a zero-width space after the trigger character, so no
// renderer reads a file or the environment on a fragment's behalf. The emitters write
// none of these themselves; a timing view's `@0` time lines keep their digit.
// Mirrors docs/compiler/diagrams.md#the-emitters.
const INERT: char = '\u{200B}';

pub(super) fn inert(line: &str) -> String {
    let body = line.trim_start();
    let indent = &line[..line.len() - body.len()];
    let mut out = String::with_capacity(line.len() + 3);
    out.push_str(indent);
    let mut head = body.chars();
    let directive = match (head.next(), head.next()) {
        (Some('!'), _) => true,
        (Some('@'), next) => !next.is_some_and(|n| n.is_ascii_digit() || n == '.'),
        _ => false,
    };
    if directive {
        out.push(INERT);
    }
    let lower = body.to_ascii_lowercase();
    let bytes = body.as_bytes();
    for (i, c) in body.char_indices() {
        out.push(c);
        if c == '<' && lower[i + 1..].starts_with("img") {
            out.push(INERT);
        }
        if c == '%'
            && bytes
                .get(i + 1)
                .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'$')
        {
            out.push(INERT);
        }
    }
    out
}

pub(super) fn document(title: Option<String>, body: Vec<String>) -> String {
    let mut s = String::from("@startuml\n");
    if let Some(t) = title {
        s.push_str(&inert(&format!("title {}", t)));
        s.push('\n');
    }
    for line in body {
        s.push_str(&inert(&line));
        s.push('\n');
    }
    s.push_str("@enduml\n");
    s
}

pub(super) fn empty_diagram(view: &View, what: &str) -> String {
    document(
        None,
        vec![format!(
            "rectangle {} as empty",
            quoted(&format!("{}: {}", view.title, what))
        )],
    )
}

pub(super) fn notation(rel_type: &str) -> &'static str {
    match rel_type {
        "generalization" => "--|>",
        "realization" => "..|>",
        "composition" => "*--",
        "aggregation" => "o--",
        "association" => "--",
        _ => "..>",
    }
}

// ---- drill-down links ----

// The PlantUML hyperlink every rendered entity carries, to its card: from any
// diagrams/<kind>/<slug>.svg, `../../docsgen/entities/<slug>.md` is the card under
// the same out directory. One link per node, collapsed or not; the card decides the
// rest. A message carries none. Mirrors docs/compiler/diagrams.md#drill-down.
pub fn card_link(id: &str) -> String {
    format!(
        "[[../../docsgen/entities/{}.md]]",
        crate::ids::entity_slug(id)
    )
}

// The suffix an element declaration carries: a space and the card link.
pub(super) fn link_suffix(id: &str) -> String {
    format!(" {}", card_link(id))
}

// `keyword "Name" as alias [<<stereotype>>] [[card link]]`
pub(super) fn declare(scene: &Scene, keyword: &str, id: &str, stereotype: bool) -> String {
    let e = scene.entity(id);
    let mut line = format!("{} {} as {}", keyword, quoted(&e.name), alias_of(id));
    // The stereotype rides on the element unless it is the keyword it became (a
    // «component» drawn as `component` says nothing twice). Mirrors
    // docs/compiler/diagrams.md#the-emitters.
    if stereotype {
        if let Some(s) = e.stereotype.as_deref() {
            if !s.eq_ignore_ascii_case(keyword) {
                line.push_str(&format!(" <<{}>>", oneline(s).replace(['<', '>'], "")));
            }
        }
    }
    line.push_str(&link_suffix(id));
    line
}

pub(super) fn with_body(out: &mut Vec<String>, line: String, body: Vec<String>) {
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

pub(super) fn arrow_line(a: &Arrow, notation: &str, label: Option<&str>) -> String {
    arrow_line_with(a, notation, label, &|id| alias_of(id))
}

// The count label stays ASCII: the crate measures edge labels byte by byte.
pub(super) fn arrow_line_with(
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

pub(super) fn arrow_lines(arrows: &[Arrow]) -> Vec<String> {
    arrows
        .iter()
        .map(|a| arrow_line(a, notation(&a.rel_type), None))
        .collect()
}

// `name : type` when the type is stated, `name = value` when a value is, else `name`.
pub(super) fn attribute_line(a: &Attribute) -> String {
    match (a.r#type.as_deref(), a.value.as_deref()) {
        (Some(t), _) => format!("{} : {}", oneline(&a.name), oneline(t)),
        (None, Some(v)) => format!("{} = {}", oneline(&a.name), oneline(v)),
        (None, None) => oneline(&a.name),
    }
}
