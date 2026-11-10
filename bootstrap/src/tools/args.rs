use super::*;

// A value that says nothing: an empty or blank string, or a list or an object whose
// every member says nothing. Numbers and booleans always say something.
pub(super) fn hollow(v: &Value) -> Option<Value> {
    match v {
        Value::Null => None,
        Value::String(s) => (!s.trim().is_empty()).then(|| v.clone()),
        Value::Array(a) => {
            let kept: Vec<Value> = a.iter().filter_map(hollow).collect();
            (!kept.is_empty()).then(|| Value::Array(kept))
        }
        Value::Object(o) => {
            let kept: serde_json::Map<String, Value> = o
                .iter()
                .filter_map(|(k, x)| hollow(x).map(|x| (k.clone(), x)))
                .collect();
            (!kept.is_empty()).then(|| Value::Object(kept))
        }
        _ => Some(v.clone()),
    }
}

// The arguments with every empty value removed, at any depth. A top-level `null`
// stays: some tools read it as "remove" (update_diagnostic's prompt). A transition
// that names no state (`from` and `to` both empty) is a filled-in blank and goes too.
// Mirrors docs/compiler/tools.md#validation-and-errors.
pub(crate) fn normalize_args(args: &Value) -> Value {
    let Value::Object(o) = args else {
        return args.clone();
    };
    let mut out = serde_json::Map::new();
    for (k, v) in o {
        if v.is_null() {
            out.insert(k.clone(), Value::Null);
        } else if let Some(v) = hollow(v) {
            out.insert(k.clone(), v);
        }
    }
    if let Some(t) = out.get("transition") {
        if t.get("from").is_none() && t.get("to").is_none() {
            out.remove("transition");
        }
    }
    Value::Object(out)
}

// A fact that arrives with its quote and a derivation both keeps the quote: the one
// provenance of a stated fact is the sentence that states it. The derivation is
// dropped and the reply says so. Mirrors docs/compiler/tools.md#validation-and-errors.
pub(super) fn prefer_quote(name: &str, args: &mut Value) -> Option<String> {
    let quoted = match name {
        "upsert_entity" => args.get("mention").is_some(),
        "upsert_requirement" | "upsert_criterion" => {
            args.get("quote").is_some() && args.get("section").is_some()
        }
        _ => false,
    };
    if !quoted || args.get("provenance").is_none() {
        return None;
    }
    args.as_object_mut()?.remove("provenance");
    let what = if name == "upsert_entity" {
        "the mention"
    } else {
        "section and quote"
    };
    Some(format!(
        "provenance dropped: the call carried {} and a derivation; a fact the documents state keeps its quote as its one provenance, so send `provenance` only for what no sentence states",
        what
    ))
}

// A statement is free-form; the only shape gate is that it says something.
pub(super) fn statement_present(statement: &str) -> Result<(), ToolError> {
    if statement.trim().is_empty() {
        return Err(ToolError::new(
            "bad-args",
            "statement is empty; state the one obligation the sentence carries".into(),
        ));
    }
    Ok(())
}

// Parse the edges argument of a requirement call: directional, typed from the catalog,
// cardinality one of the four forms. `entities` restricts the ends when given.
pub(super) fn parse_edges(
    session: &ToolSession,
    arr: &[Value],
    entities: Option<&[String]>,
) -> Result<Vec<ReqEdge>, ToolError> {
    let mut edges = Vec::new();
    for e in arr {
        // Empty means absent (docs/compiler/tools.md#validation-and-errors): an
        // all-empty item is a filled-in blank, not an edge.
        if !ToolSession::present(e) {
            continue;
        }
        let raw_a = e["a"].as_str().unwrap_or_default();
        let raw_b = e["b"].as_str().unwrap_or_default();
        let a = session
            .canon_entity_id(raw_a)
            .unwrap_or_else(|| raw_a.to_string());
        let b = session
            .canon_entity_id(raw_b)
            .unwrap_or_else(|| raw_b.to_string());
        if let Some(listed) = entities {
            if !listed.contains(&a) || !listed.contains(&b) {
                return Err(ToolError::new(
                    "bad-edge",
                    format!(
                        "edge {}~{} may only tie entities the requirement itself references",
                        a, b
                    ),
                ));
            }
        }
        let t = e["type"]
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(t) = &t {
            if !REL_TYPES.contains(&t.as_str()) {
                return Err(ToolError::new(
                    "bad-edge",
                    format!(
                        "unknown relationship type `{}`; one of: {}",
                        t,
                        REL_TYPES.join(", ")
                    ),
                ));
            }
        }
        let cardinality = e["cardinality"]
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(c) = &cardinality {
            if !crate::model::is_cardinality(c) {
                return Err(ToolError::new(
                    "bad-cardinality",
                    format!(
                        "unknown cardinality `{}`; a multiplicity: *, a count (1, 4), or a range (0..1, 1..*, 2..3)",
                        c
                    ),
                ));
            }
        }
        if a == b {
            return Err(ToolError::new(
                "bad-edge",
                format!(
                    "edge {}~{} ties an entity to itself; a and b are distinct",
                    a, b
                ),
            ));
        }
        // Edges are directional: the same pair in the other direction, or under
        // another type, is another edge. The same (a, b, type) again is the same one.
        if edges
            .iter()
            .any(|x: &ReqEdge| x.a == a && x.b == b && x.rel_type == t)
        {
            continue;
        }
        edges.push(ReqEdge {
            a,
            b,
            rel_type: t,
            cardinality,
        });
    }
    Ok(edges)
}

// Parse the facets argument: each names a facet from the catalog with its reasoning;
// `measure` is accepted only on `quality`. Mirrors docs/compiler/model/requirement.md#facets.
pub(super) fn parse_facets(v: &Value) -> Result<Vec<Facet>, ToolError> {
    let Some(arr) = v.as_array() else {
        return Err(ToolError::new(
            "bad-facet",
            "facets is a list of {facet, reasoning, measure?}".into(),
        ));
    };
    let mut out: Vec<Facet> = Vec::new();
    for f in arr {
        // Empty means absent (docs/compiler/tools.md#validation-and-errors): an
        // all-empty item is a filled-in blank, not a facet.
        if !ToolSession::present(f) {
            continue;
        }
        let facet = f["facet"].as_str().unwrap_or_default().trim().to_string();
        if !FACETS.contains(&facet.as_str()) {
            return Err(ToolError::new(
                "bad-facet",
                format!("unknown facet `{}`; one of: {}", facet, FACETS.join(", ")),
            ));
        }
        let reasoning = f["reasoning"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        if reasoning.is_empty() {
            return Err(ToolError::new(
                "bad-facet",
                format!("facet `{}` needs its reasoning", facet),
            ));
        }
        let measure = f["measure"]
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if measure.is_some() && facet != "quality" {
            return Err(ToolError::new(
                "bad-facet",
                format!(
                    "measure is stated only on a quality facet, not on `{}`",
                    facet
                ),
            ));
        }
        if let Some(mine) = out.iter_mut().find(|x| x.facet == facet) {
            mine.reasoning = reasoning;
            mine.measure = measure;
            continue;
        }
        out.push(Facet {
            facet,
            reasoning,
            measure,
        });
    }
    Ok(out)
}

// The membership rule of a view kind: what its members are.
// Mirrors docs/compiler/model/view.md#kinds.
pub(super) fn member_rule(kind: &str) -> &'static str {
    match kind {
        "class" | "object" | "package" | "component" | "deployment" => "entities",
        "composite" | "state" => "one entity",
        "timing" => "one entity then requirements",
        _ => "requirements",
    }
}

// Names that look like syntax rather than a concept. Rejected without an explaining note.
pub(super) fn junk_name(name: &str) -> Option<&'static str> {
    let n = name.trim();
    let lower = n.to_lowercase();
    if n.starts_with('-') {
        return Some("looks like a CLI flag");
    }
    if n.contains('/') || n.contains('\\') {
        return Some("looks like a file path");
    }
    for ext in [".md", ".rs", ".yaml", ".yml", ".toml", ".json", ".html"] {
        if lower.ends_with(ext) {
            return Some("looks like a file name");
        }
    }
    if n.contains('`') || n.contains('#') {
        return Some("contains markup");
    }
    // A single camelCase token is a code identifier: an operation or accessor named in
    // the docs. Operations are requirement detail, never entities.
    if !n.contains(' ')
        && n.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && n.chars().any(|c| c.is_ascii_uppercase())
    {
        return Some("looks like a code identifier (an operation or function); operations belong in the requirement statement, not the entity list");
    }
    const MD_TERMS: [&str; 12] = [
        "heading",
        "headings",
        "code block",
        "code blocks",
        "blockquote",
        "blockquotes",
        "list item",
        "list items",
        "markdown",
        "table",
        "link",
        "bullet",
    ];
    if MD_TERMS.contains(&lower.as_str()) {
        return Some("is a markdown construct, not a domain concept");
    }
    for article in ["the ", "a ", "an "] {
        if lower.starts_with(article) {
            return Some("starts with an article; name the concept itself");
        }
    }
    if n.len() > 60 {
        return Some("too long for a concept name");
    }
    if n.is_empty() {
        return Some("empty");
    }
    None
}

impl ToolSession {
    // Parse a transition argument: the subject resolves, is among the requirement's
    // entities, and the two states are named. Mirrors docs/compiler/model/requirement.md#transition.
    pub(super) fn parse_transition(
        &self,
        v: &Value,
        entities: &[String],
    ) -> Result<Transition, ToolError> {
        if !v.is_object() {
            return Err(ToolError::new(
                "bad-transition",
                "transition is {subject, from, to, trigger?, guard?}".into(),
            ));
        }
        let raw = v["subject"].as_str().unwrap_or_default().trim();
        if raw.is_empty() {
            return Err(ToolError::new(
                "bad-transition",
                "transition.subject is empty; name the subject entity, or omit `transition` entirely"
                    .into(),
            ));
        }
        let Some(subject) = self.canon_entity_id(raw) else {
            let e = self.unknown_entity_error(raw);
            return Err(ToolError::new(
                "unknown-id",
                format!("transition subject: {}", e.message),
            ));
        };
        if !entities.contains(&subject) {
            return Err(ToolError::new(
                "bad-transition",
                format!(
                    "transition subject {} is not among the requirement's entities ({}); list it there",
                    subject,
                    entities.join(", ")
                ),
            ));
        }
        let state = |k: &str| -> Result<String, ToolError> {
            v[k].as_str()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    let coach = if k == "from" {
                        " Name the state the documents place the subject in before the trigger; the event that creates the subject names its initial state. When no sentence states one, omit `transition` entirely."
                    } else {
                        ""
                    };
                    ToolError::new(
                        "bad-transition",
                        format!("transition.{} names a state; it is empty.{}", k, coach),
                    )
                })
        };
        let opt = |k: &str| {
            v[k].as_str()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        };
        Ok(Transition {
            subject,
            from: state("from")?,
            to: state("to")?,
            trigger: opt("trigger"),
            guard: opt("guard"),
        })
    }

    // Parse an attributes argument: named, unique by name, each with a located quote
    // provenance or the call's own. Mirrors docs/compiler/graph.md#validation-gates.
    pub(super) fn parse_attributes(
        &self,
        v: &Value,
        default: Option<&SourceRef>,
    ) -> Result<Vec<Attribute>, ToolError> {
        let Some(arr) = v.as_array() else {
            return Err(ToolError::new(
                "bad-attribute",
                "attributes is a list of {name, type?, value?, provenance?: {section, quote}}"
                    .into(),
            ));
        };
        let mut out: Vec<Attribute> = Vec::new();
        for a in arr {
            // Empty means absent (docs/compiler/tools.md#validation-and-errors): an
            // all-empty item is a filled-in blank, not an attribute.
            if !Self::present(a) {
                continue;
            }
            let name = a["name"].as_str().unwrap_or_default().trim().to_string();
            if name.is_empty() {
                return Err(ToolError::new(
                    "bad-attribute",
                    "an attribute needs a name".into(),
                ));
            }
            if out.iter().any(|x| x.name == name) {
                return Err(ToolError::new(
                    "bad-attribute",
                    format!(
                        "attribute `{}` is listed twice; attributes are unique by name",
                        name
                    ),
                ));
            }
            let opt = |k: &str| {
                a[k].as_str()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            };
            // A hollow provenance object counts as absent: the attribute falls
            // back to the call's own quote, as the schema promises.
            let provenance = if Self::present(&a["provenance"]) {
                let section = a["provenance"]["section"].as_str().unwrap_or_default();
                let quote = a["provenance"]["quote"].as_str().unwrap_or_default();
                let (doc, sec) = self.resolve_section(section)?;
                let quote = self.check_quote(&doc, &sec, quote)?;
                Provenance::Quote(SourceRef {
                    doc,
                    section: sec,
                    quote,
                })
            } else {
                match default {
                    Some(src) => Provenance::Quote(src.clone()),
                    None => {
                        return Err(ToolError::new(
                            "bad-attribute",
                            format!(
                                "attribute `{}` needs a provenance ({{section, quote}}); the call carries no quote it could take",
                                name
                            ),
                        ))
                    }
                }
            };
            out.push(Attribute {
                name,
                r#type: opt("type"),
                value: opt("value"),
                provenance,
            });
        }
        Ok(out)
    }

    // An optional argument that is empty counts as absent: a model that fills every
    // schema field with "" / [] / {} makes the same call as one that omits them.
    // Mirrors docs/compiler/tools.md#validation-and-errors.
    pub(super) fn present(v: &Value) -> bool {
        match v {
            Value::Null => false,
            Value::String(s) => !s.trim().is_empty(),
            Value::Array(a) => a.iter().any(Self::present),
            Value::Object(o) => o.values().any(Self::present),
            _ => true,
        }
    }

    // Parse a provenance argument a session may stage: a derivation naming live nodes
    // with its reasoning. A decree is refused: only a human path stages one.
    pub(super) fn parse_derived(&self, v: &Value) -> Result<Provenance, ToolError> {
        if v["decree"].is_object() {
            return Err(ToolError::new(
                "bad-provenance",
                "a session never stages a decree; pass provenance.derived {from, reasoning}, or a mention".into(),
            ));
        }
        let d = &v["derived"];
        if !d.is_object() {
            return Err(ToolError::new(
                "bad-provenance",
                "provenance is {derived: {from: [ids], reasoning}}".into(),
            ));
        }
        let mut from: Vec<String> = Vec::new();
        for raw in Self::str_list(d, "from") {
            let Some(id) = self.node_known(&raw) else {
                // A section reference is no node: the fact it states is quoted.
                // Mirrors docs/compiler/model.md#provenance.
                if raw.contains('#') {
                    return Err(ToolError::new(
                        "unknown-id",
                        format!(
                            "provenance.derived.from names `{}`, a section, and from names graph nodes (ent:, req:, view: ids); a fact the section states enters as mention {{section, quote}} with a body sentence, and a heading is no body sentence",
                            raw
                        ),
                    ));
                }
                return Err(ToolError::new(
                    "unknown-id",
                    format!(
                        "provenance.derived.from names `{}`, which does not exist",
                        raw
                    ),
                ));
            };
            if !from.contains(&id) {
                from.push(id);
            }
        }
        if from.is_empty() {
            return Err(ToolError::new(
                "bad-provenance",
                "provenance.derived.from names at least one live node".into(),
            ));
        }
        let reasoning = d["reasoning"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        if reasoning.is_empty() {
            return Err(ToolError::new(
                "bad-provenance",
                "provenance.derived.reasoning says why the fact holds".into(),
            ));
        }
        Ok(Provenance::Derived { from, reasoning })
    }

    pub(super) fn str_arg(args: &Value, key: &str) -> Result<String, ToolError> {
        args[key]
            .as_str()
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                ToolError::new(
                    "bad-args",
                    format!("missing required string argument `{}`", key),
                )
            })
    }

    pub(super) fn opt_str(args: &Value, key: &str) -> Option<String> {
        args[key]
            .as_str()
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty())
    }

    // The owner recorded for tasks a batch adds: its binding, generation, or
    // verification goals. Other batches and scopes add the author's tasks.
    // Mirrors docs/consumers/gen.md#session-tasks-are-swept-when-unused.
    pub(super) fn task_owner(&self) -> Option<String> {
        let goals: Vec<&str> = self
            .scope
            .goals
            .iter()
            .filter(|g| matches!(g.kind.as_str(), "bind" | "generate" | "verify"))
            .map(|g| g.goal.as_str())
            .collect();
        (!goals.is_empty()).then(|| goals.join(","))
    }

    pub(super) fn str_list(args: &Value, key: &str) -> Vec<String> {
        // Empty means absent (docs/compiler/tools.md#validation-and-errors): every
        // caller is an id or name list, where "" is a filled-in blank, never data.
        args[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .filter(|s| !s.trim().is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }
}
