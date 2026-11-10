use super::*;

// ---- shared derivation helpers ----

pub(super) fn records_of<'a>(store: &'a Store, kinds: &[&str]) -> Vec<&'a ChangeRecord> {
    store
        .status
        .changes
        .iter()
        .filter(|c| kinds.contains(&c.kind.as_str()))
        .collect()
}

pub(super) fn earliest_cause(records: &[&ChangeRecord]) -> Option<Cause> {
    records
        .iter()
        .min_by_key(|c| (c.generation, c.mutation))
        .map(|c| c.cause())
}

pub(super) fn build_goal(
    kind: &dyn GoalKind,
    target: &str,
    mandatory: bool,
    change: Value,
    cause: Option<Cause>,
    hints: Vec<String>,
) -> Goal {
    Goal {
        id: goal_id(kind.kind(), target),
        kind: kind.kind().to_string(),
        class: kind.class().name().to_string(),
        mandatory,
        target: target.to_string(),
        unit: kind.unit().to_string(),
        change,
        cause,
        state: GoalState::Open,
        hints,
    }
}

// A section that owes coverage of its own: one with a body beyond its heading line,
// and never a code block, whose parent's mark covers it (the parent keeps its whole
// body). A diagram section owes its own. Mirrors docs/compiler/compilation.md#coverage.
// Whether a section owes a coverage mark and a reconcile goal: it has a body of its
// own. A heading section's body is its text after the heading line, a diagram's the
// block inside its fence, a preamble's its whole text (it has no heading line, so a
// one-line preamble owes its mark). A code block follows its parent's mark.
// Mirrors docs/compiler/compilation.md#coverage.
// The section that owns a reference's goal and mark: a fenced block's holding section
// (walking up through nested blocks), any other section itself.
// Mirrors docs/compiler/parsing.md#section-tree.
pub fn owning_section(store: &Store, doc: &str, sec: &str) -> String {
    let mut cur = sec.to_string();
    let Some(rec) = store.docs.get(doc) else {
        return cur;
    };
    for _ in 0..16 {
        match rec.sections.get(&cur) {
            Some(s) if s.kind == "code-block" => match &s.parent {
                Some(p) => cur = p.clone(),
                None => break,
            },
            _ => break,
        }
    }
    cur
}

pub fn section_has_body(sec: &Section) -> bool {
    if sec.kind == "blob" {
        return true;
    }
    if sec.kind == "code-block" {
        return false;
    }
    let skip = if sec.kind == "preamble" { 0 } else { 1 };
    !sec.raw.lines().skip(skip).all(|l| l.trim().is_empty())
}

pub(super) fn str_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn truncate(s: &str, n: usize) -> String {
    crate::llm::truncate(s, n)
}

pub(super) fn claimed_after(claimed: &Option<String>, generation: u64) -> bool {
    claimed
        .as_deref()
        .and_then(|b| b.strip_prefix('g'))
        .and_then(|n| n.parse::<u64>().ok())
        .is_some_and(|n| n >= generation)
}

// Content tokens of a statement: normalized tokens minus stop words and the named
// entities' own name tokens, crudely stemmed. The pair scorer's vocabulary.
pub fn content_tokens(store: &Store, statement: &str, entities: &[String]) -> BTreeSet<String> {
    content_token_words(store, statement, entities)
        .into_iter()
        .map(|(stemmed, _)| stemmed)
        .collect()
}

// The content tokens of a statement in order, each stem paired with the word the
// statement wrote, for a hint that shows a word and never a stem.
pub fn content_token_words(
    store: &Store,
    statement: &str,
    entities: &[String],
) -> Vec<(String, String)> {
    store.content_token_words(statement, entities, store::suffix_stem)
}

// Name tokens of an entity: name and aliases, normalized, stop words dropped, stemmed.
pub fn name_tokens(e: &Entity) -> BTreeSet<String> {
    const STOP: [&str; 5] = ["the", "a", "an", "of", "and"];
    std::iter::once(&e.name)
        .chain(e.aliases.iter())
        .flat_map(|n| {
            store::normalize_statement(n)
                .split(' ')
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .filter(|t| !t.is_empty() && !STOP.contains(&t.as_str()))
        .map(|t| store::suffix_stem(&t))
        .collect()
}

// The open judged diagnostics naming any of the ids. The build's own bookkeeping
// (incomplete-build, ratification-pending, unstable-*) is left out: a session never
// resolves it. Mirrors docs/compiler/goals/review-entity.md#hints.
pub(super) fn open_diags_naming<'a>(
    store: &'a Store,
    ids: &[&str],
) -> Vec<(&'a String, &'a Diagnostic)> {
    store
        .graph
        .diagnostics
        .iter()
        .filter(|(_, d)| {
            d.lifecycle == "open"
                && !crate::tools::harness_owned_rule(&d.rule)
                && d.subjects.iter().any(|s| ids.contains(&s.as_str()))
        })
        .collect()
}

pub(super) fn node_label(store: &Store, id: &str) -> String {
    if let Some(e) = store.graph.entities.get(id) {
        return format!("{} ({})", id, e.name);
    }
    if let Some(r) = store.graph.requirements.get(id) {
        return format!("{} \"{}\"", id, truncate(&r.statement, 80));
    }
    if let Some(v) = store.graph.views.get(id) {
        return format!("{} ({})", id, v.title);
    }
    id.to_string()
}
