use super::*;

// Sections of a reconcile goal with no coverage mark, recorded or staged.
pub fn unmarked_sections(
    store: &Store,
    staged: &[Op],
    doc: &str,
    sections: &[String],
) -> Vec<String> {
    if staged
        .iter()
        .any(|op| matches!(op, Op::ClassifyInput { doc: d, .. } if d == doc))
    {
        return vec![];
    }
    sections
        .iter()
        .filter(|sec| {
            let recorded = store
                .docs
                .get(doc)
                .map(|d| d.coverage.contains_key(*sec))
                .unwrap_or(false);
            let staged = staged.iter().any(|o| {
                matches!(o, Op::SetCoverage { doc: d, section, .. } if d == doc && section == *sec)
            });
            !recorded && !staged
        })
        .cloned()
        .collect()
}

// A `covered` claim is honest only when a requirement is sourced from that
// section, staged or recorded.
pub fn dishonest_covered(store: &Store, staged: &[Op]) -> Option<Violation> {
    for op in staged {
        let Op::SetCoverage {
            doc,
            section,
            state,
            ..
        } = op
        else {
            continue;
        };
        if state != "covered" {
            continue;
        }
        if crate::input::is_blob(store, doc, section) {
            if let Some(v) = ReconcileSection.gates(store, staged).into_iter().next() {
                return Some(v);
            }
            continue;
        }
        // A test-case or examples section is covered by the criterion sourced
        // from it (docs/compiler/goals/reconcile-section.md#gate).
        let crit_at = |s: &Option<SourceRef>| {
            s.as_ref()
                .is_some_and(|s| s.doc == *doc && s.section == *section)
        };
        let has_req = store
            .graph
            .requirements
            .values()
            .any(|r| r.anchored_at(doc, section))
            || store.graph.criteria.values().any(|c| crit_at(&c.source))
            || staged.iter().any(|o| match o {
                Op::CreateRequirement { requirement, .. } => requirement.anchored_at(doc, section),
                Op::CreateCriterion { criterion, .. } => crit_at(&criterion.source),
                _ => false,
            });
        if !has_req {
            return Some(Violation::new(
                "uncovered-claim",
                format!(
                    "{}#{} is claimed covered but no requirement is sourced from it; extract from its sentences (state the obligation, keep the quote verbatim), or mark the section non-normative with a note",
                    doc, section
                ),
            ));
        }
    }
    None
}

// ---- reconcile-section ----

pub struct ReconcileSection;

#[derive(Default)]
struct SectionWork {
    dirty: Option<Value>,
    stale: BTreeSet<String>,
    unprocessed: bool,
    causes: Vec<Cause>,
}

// How many requirements are sourced in each section (`doc#section`), one pass over
// the requirements.
fn requirements_per_section(store: &Store) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for r in store.graph.requirements.values() {
        if let Some(s) = &r.source {
            *out.entry(format!("{}#{}", s.doc, s.section)).or_insert(0) += 1;
        }
    }
    out
}

// The hint lines every section goal of one document shares: its subjects and the
// known entities its text names. Computed once per document, not per section (a
// store with two hundred dirty sections once lowercased its documents and searched
// two thousand names two hundred times).
struct DocHintLines {
    subjects: Option<String>,
    known: String,
}

// Every entity's name and aliases lowercased, the ones three characters or longer,
// by id: what "named in the document" searches the text for, prepared once per
// derivation rather than once per document per entity.
struct EntityNames(BTreeMap<String, (String, Vec<String>)>);

impl EntityNames {
    fn build(store: &Store) -> EntityNames {
        EntityNames(
            store
                .graph
                .entities
                .iter()
                .map(|(id, e)| {
                    let lc: Vec<String> = std::iter::once(&e.name)
                        .chain(e.aliases.iter())
                        .filter(|n| n.len() >= 3)
                        .map(|n| n.to_lowercase())
                        .collect();
                    (id.clone(), (e.name.clone(), lc))
                })
                .collect(),
        )
    }

    fn named_in(&self, id: &str, text: &str) -> bool {
        self.0
            .get(id)
            .is_some_and(|(_, lc)| lc.iter().any(|n| text.contains(n.as_str())))
    }
}

fn doc_hint_lines(
    store: &Store,
    doc: &str,
    linked: &[(String, String, String)],
    names: &EntityNames,
) -> DocHintLines {
    // The document's own text decides what it names: an entity linked from
    // another document counts as this one's subject only when this document
    // names it (or is titled with its name), and the known entities are the
    // ones whose name or alias occurs in the text, listed by id. A settings
    // document once read "primarySubject: Scene; 0 entities mentioned" because
    // a scene's setting linked to it and mentions were counted, not names.
    // Mirrors docs/compiler/goals/reconcile-section.md#what-the-model-sees.
    let text = store
        .docs
        .get(doc)
        .map(|d| {
            d.sections
                .values()
                .map(|s| s.raw.to_lowercase())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let root_title = store.docs.get(doc).and_then(|d| {
        d.sections
            .values()
            .find(|s| s.kind == "root")
            .map(|s| s.title.to_lowercase())
    });
    let subjects: Vec<&(String, String, String)> = linked
        .iter()
        .filter(|(id, name, _)| {
            root_title.as_deref() == Some(name.to_lowercase().as_str()) || names.named_in(id, &text)
        })
        .collect();
    let subjects = match subjects.len() {
        0 if !linked.is_empty() => Some(format!(
            "linked from {} other document(s) (via {}); none of their entities is named here, so no primarySubject",
            linked.len(),
            linked
                .iter()
                .map(|(id, _, _)| id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
        0 => None,
        1 => Some(format!(
            "primarySubject: {} ({})",
            subjects[0].0, subjects[0].1
        )),
        _ => Some(format!(
            "candidateSubjects: {}",
            subjects
                .iter()
                .map(|(id, name, _)| format!("{} ({})", id, name))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    };
    let mut known: Vec<String> = names
        .0
        .iter()
        .filter(|(_, (_, lc))| lc.iter().any(|n| text.contains(n.as_str())))
        .map(|(id, (name, _))| format!("{} ({})", name, id))
        .collect();
    let known_count = known.len();
    known.truncate(8);
    if known_count > 8 {
        known.push(format!("and {} more", known_count - 8));
    }
    let known = if store.graph.entities.is_empty() {
        "the graph holds no entity yet: create what the section states, no search can find anything"
            .to_string()
    } else if known_count == 0 {
        format!(
            "no known entity is named in the document; {} in the graph; search before creating",
            store.graph.entities.len()
        )
    } else {
        format!(
            "{} known entities named in the document: {}; {} in the graph; search before creating",
            known_count,
            known.join(", "),
            store.graph.entities.len()
        )
    };
    DocHintLines { subjects, known }
}

// Whether a stale anchor still owes work: its quote fails to locate or it is flagged
// for re-evaluation. A deleted anchor owes nothing.
fn anchor_stale(store: &Store, id: &str) -> bool {
    if store.status.reevaluate.iter().any(|x| x == id) {
        return true;
    }
    if let Some(r) = store.graph.requirements.get(id) {
        return r
            .source
            .as_ref()
            .map(|q| !store.quote_locates(&q.doc, &q.section, &q.quote))
            .unwrap_or(false);
    }
    if let Some(e) = store.graph.entities.get(id) {
        return e
            .mentions
            .iter()
            .any(|m| !store.quote_locates(&m.doc, &m.section, &m.quote));
    }
    false
}

// Entities another document introduced for this one: a mention or a requirement
// quote elsewhere whose link resolves to the document.
pub fn linked_subjects(store: &Store, doc: &str) -> Vec<(String, String, String)> {
    linked_subjects_all(store).remove(doc).unwrap_or_default()
}

// The linked subjects of every document in one pass: each mention's and each
// quote's links are parsed once and credited to every document they resolve to, in
// the order the per-document scan would list them, twelve per document. A per-section
// hint once parsed every link in the store again for every dirty section.
pub fn linked_subjects_all(store: &Store) -> BTreeMap<String, Vec<(String, String, String)>> {
    let mut out: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
    // The sentence carrying the link must name the entity it introduces; a scene's
    // setting line linking to the settings page introduces no Scene.
    // Mirrors docs/compiler/goals/reconcile-section.md#incoming-links.
    let names = |e: &Entity, quote: &str| {
        let q = quote.to_lowercase();
        std::iter::once(&e.name)
            .chain(e.aliases.iter())
            .any(|n| n.len() >= 3 && q.contains(&n.to_lowercase()))
    };
    let listed = |out: &BTreeMap<String, Vec<(String, String, String)>>, doc: &str, id: &str| {
        out.get(doc)
            .is_some_and(|v| v.iter().any(|(x, _, _)| x == id))
    };
    for (id, e) in &store.graph.entities {
        for m in &e.mentions {
            if !names(e, &m.quote) {
                continue;
            }
            for l in crate::md::doc_links(&m.quote, &m.doc) {
                // One mention per entity per document: the first that links there.
                if l == m.doc || listed(&out, &l, id) {
                    continue;
                }
                out.entry(l).or_default().push((
                    id.clone(),
                    e.name.clone(),
                    format!("{}#{}", m.doc, m.section),
                ));
            }
        }
    }
    for r in store.graph.requirements.values() {
        let Some(src) = r.source.as_ref() else {
            continue;
        };
        for l in crate::md::doc_links(&src.quote, &src.doc) {
            if l == src.doc {
                continue;
            }
            for e in &r.entities {
                let id = store.resolve_id(e).to_string();
                if listed(&out, &l, &id) {
                    continue;
                }
                if let Some(ent) = store.graph.entities.get(&id) {
                    if names(ent, &src.quote) {
                        out.entry(l.clone()).or_default().push((
                            id,
                            ent.name.clone(),
                            format!("{}#{}", src.doc, src.section),
                        ));
                    }
                }
            }
        }
    }
    for v in out.values_mut() {
        v.truncate(12);
    }
    out
}

// The linked subjects of one document by the plain scan: the reference the one-pass
// index is checked against.
#[cfg(test)]
pub(crate) fn linked_subjects_scan(store: &Store, doc: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    // The sentence carrying the link must name the entity it introduces; a scene's
    // setting line linking to the settings page introduces no Scene.
    // Mirrors docs/compiler/goals/reconcile-section.md#incoming-links.
    let names = |e: &Entity, quote: &str| {
        let q = quote.to_lowercase();
        std::iter::once(&e.name)
            .chain(e.aliases.iter())
            .any(|n| n.len() >= 3 && q.contains(&n.to_lowercase()))
    };
    for (id, e) in &store.graph.entities {
        for m in &e.mentions {
            if m.doc == doc || !names(e, &m.quote) {
                continue;
            }
            if crate::md::doc_links(&m.quote, &m.doc)
                .iter()
                .any(|l| l == doc)
            {
                out.push((
                    id.clone(),
                    e.name.clone(),
                    format!("{}#{}", m.doc, m.section),
                ));
                break;
            }
        }
    }
    for r in store.graph.requirements.values() {
        let Some(s) = r.source.as_ref() else { continue };
        if s.doc == doc
            || !crate::md::doc_links(&s.quote, &s.doc)
                .iter()
                .any(|l| l == doc)
        {
            continue;
        }
        for e in &r.entities {
            let id = store.resolve_id(e).to_string();
            if out.iter().any(|(x, _, _)| *x == id) {
                continue;
            }
            if let Some(ent) = store.graph.entities.get(&id) {
                if names(ent, &s.quote) {
                    out.push((id, ent.name.clone(), format!("{}#{}", s.doc, s.section)));
                }
            }
        }
    }
    out.truncate(12);
    out
}

// Folds one `section-dirty` record's detail into the goal's: the line lists join
// without repeats, an added line the body no longer holds drops out, and each count is
// its list's length plus the lines the record cap left out.
// Mirrors docs/compiler/goals/reconcile-section.md#created-when.
fn fold_dirty_detail(acc: Option<Value>, rec: &Value, body: &str) -> Value {
    // Lines a record counted but did not list (past the cap).
    let overflow = |d: &Value, count: &str, list: &str| {
        d[count]
            .as_u64()
            .unwrap_or(0)
            .saturating_sub(str_list(&d[list]).len() as u64)
    };
    let mut out = match &acc {
        None => rec.clone(),
        Some(a) => {
            let mut out = a.clone();
            if out.get("previous").is_none() {
                if let Some(p) = rec.get("previous") {
                    out["previous"] = p.clone();
                }
            }
            out
        }
    };
    let in_body: BTreeSet<&str> = body.lines().map(str::trim).collect();
    for (count, list) in [("added", "addedLines"), ("removed", "removedLines")] {
        if rec.get(list).is_none() && acc.as_ref().is_none_or(|a| a.get(list).is_none()) {
            continue;
        }
        let mut lines = acc.as_ref().map(|a| str_list(&a[list])).unwrap_or_default();
        for l in str_list(&rec[list]) {
            if !lines.contains(&l) {
                lines.push(l);
            }
        }
        if list == "addedLines" {
            lines.retain(|l| in_body.contains(l.as_str()));
        }
        let extra = overflow(rec, count, list)
            + acc.as_ref().map(|a| overflow(a, count, list)).unwrap_or(0);
        out[count] = json!(lines.len() as u64 + extra);
        out[list] = json!(lines);
    }
    out
}

// The added prose lines of a section that nothing in the session accounts for: no
// staged quote for the section overlaps the line, and the coverage note does not repeat
// it. Mirrors docs/compiler/goals/reconcile-section.md#gate.
fn unaccounted_added_lines(
    store: &Store,
    staged: &[Op],
    doc: &str,
    section: &str,
    note: &str,
) -> Vec<String> {
    let added = section_added_lines(store, doc, section);
    if added.is_empty() {
        return Vec::new();
    }
    let mut quotes = Vec::new();
    for op in staged {
        if let Ok(v) = serde_json::to_value(op) {
            staged_quotes(&v, doc, section, &mut quotes);
        }
    }
    added
        .into_iter()
        .filter(|l| {
            let text = prose_text(l);
            if text.is_empty() {
                return false;
            }
            let carried = quotes
                .iter()
                .any(|q| store::text_contains(&text, q) || store::text_contains(q, &text));
            !carried && !store::text_contains(note, &text)
        })
        .collect()
}

// Every quote in a staged op that names the section, or names no section at all.
fn staged_quotes(v: &Value, doc: &str, section: &str, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            if let Some(q) = o.get("quote").and_then(Value::as_str) {
                let doc_ok = o
                    .get("doc")
                    .and_then(Value::as_str)
                    .is_none_or(|d| d == doc);
                let sec_ok = o
                    .get("section")
                    .and_then(Value::as_str)
                    .is_none_or(|s| s == section);
                if doc_ok && sec_ok {
                    out.push(q.to_string());
                }
            }
            for v in o.values() {
                staged_quotes(v, doc, section, out);
            }
        }
        Value::Array(a) => {
            for v in a {
                staged_quotes(v, doc, section, out);
            }
        }
        _ => {}
    }
}

// An added line as prose: the markdown prefix (quote marker, bullet, ordered number)
// stripped; empty for a heading, a fence, or a line without a letter or digit.
fn prose_text(line: &str) -> String {
    let t = line.trim();
    if t.starts_with('#') || t.starts_with("```") || t.starts_with("~~~") {
        return String::new();
    }
    let mut t = t;
    loop {
        let before = t;
        t = t.trim_start_matches(['>', '-', '*', '+', '|']).trim_start();
        let digits = t.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && t[digits..].starts_with(['.', ')']) {
            t = t[digits + 1..].trim_start();
        }
        if t == before {
            break;
        }
    }
    if !t.chars().any(char::is_alphanumeric) {
        return String::new();
    }
    t.to_string()
}

// The diff a section's records carry, folded: (added lines still in the body, removed
// lines). None when no record carries line lists (a record written before they did).
pub fn section_diff(store: &Store, doc: &str, section: &str) -> Option<(Vec<String>, Vec<String>)> {
    let d = section_dirty_detail(store, doc, section)?;
    if d.get("addedLines").is_none() && d.get("removedLines").is_none() {
        return None;
    }
    Some((str_list(&d["addedLines"]), str_list(&d["removedLines"])))
}

// The lines an edit added to a section, still in its body, as its records carry them.
pub fn section_added_lines(store: &Store, doc: &str, section: &str) -> Vec<String> {
    section_dirty_detail(store, doc, section)
        .map(|d| str_list(&d["addedLines"]))
        .unwrap_or_default()
}

fn section_dirty_detail(store: &Store, doc: &str, section: &str) -> Option<Value> {
    let full = format!("{}#{}", doc, section);
    let body = store
        .docs
        .get(doc)
        .and_then(|r| r.sections.get(section))
        .map(|s| s.raw.as_str())
        .unwrap_or("");
    let claimed = store
        .docs
        .get(doc)
        .and_then(|r| r.coverage.get(section))
        .map(|c| c.claimed_by.clone());
    let mut acc: Option<Value> = None;
    for rec in records_of(store, &[store::CHANGE_SECTION_DIRTY]) {
        // A record a later coverage claim already answered is spent, as in derivation.
        if claimed
            .as_ref()
            .is_some_and(|c| claimed_after(c, rec.generation))
        {
            continue;
        }
        if rec.subject == full {
            acc = Some(fold_dirty_detail(acc, &rec.detail, body));
        }
    }
    acc
}

impl GoalKind for ReconcileSection {
    fn kind(&self) -> &'static str {
        "reconcile-section"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "section"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut work: BTreeMap<String, SectionWork> = BTreeMap::new();
        for rec in records_of(store, &[store::CHANGE_SECTION_DIRTY]) {
            let Some((doc, sec)) = split_section_ref(&rec.subject) else {
                continue;
            };
            let Some(record) = store.docs.get(&doc) else {
                continue;
            };
            if !record.sections.contains_key(&sec) {
                continue;
            }
            // A fenced block's dirt is its holding section's: the block owns no goal.
            // Mirrors docs/compiler/parsing.md#section-tree.
            let sec = owning_section(store, &doc, &sec);
            // A bare heading has nothing to extract and nothing to mark.
            // Mirrors docs/compiler/goals/reconcile-section.md#created-when.
            if record
                .sections
                .get(&sec)
                .is_some_and(|s| !section_has_body(s))
            {
                continue;
            }
            if record
                .coverage
                .get(&sec)
                .is_some_and(|c| claimed_after(&c.claimed_by, rec.generation))
            {
                continue;
            }
            let body = record
                .sections
                .get(&sec)
                .map(|s| s.raw.as_str())
                .unwrap_or("");
            let w = work.entry(format!("{}#{}", doc, sec)).or_default();
            w.dirty = Some(fold_dirty_detail(w.dirty.take(), &rec.detail, body));
            w.causes.push(rec.cause());
        }
        for rec in records_of(store, &[store::CHANGE_ANCHOR_STALE]) {
            let Some((doc, sec)) = split_section_ref(&rec.subject) else {
                continue;
            };
            if !store
                .docs
                .get(&doc)
                .is_some_and(|r| r.sections.contains_key(&sec))
            {
                continue;
            }
            let anchors: Vec<String> = str_list(&rec.detail["anchors"])
                .into_iter()
                .filter(|a| anchor_stale(store, a))
                .collect();
            if anchors.is_empty() {
                continue;
            }
            let sec = owning_section(store, &doc, &sec);
            let w = work.entry(format!("{}#{}", doc, sec)).or_default();
            w.stale.extend(anchors);
            w.causes.push(rec.cause());
        }
        // Anchors whose quote stopped locating, or flagged by a placement, ride on
        // their section whether or not a record names them.
        for doc in store.docs.keys() {
            let (stale, _) = store.stale_extras(doc);
            for rid in stale {
                let Some(src) = store
                    .graph
                    .requirements
                    .get(&rid)
                    .and_then(|r| r.source.as_ref())
                else {
                    continue;
                };
                if !store
                    .docs
                    .get(&src.doc)
                    .is_some_and(|r| r.sections.contains_key(&src.section))
                {
                    continue;
                }
                let sec = owning_section(store, &src.doc, &src.section);
                let full = format!("{}#{}", src.doc, sec);
                let w = work.entry(full).or_default();
                if w.stale.insert(rid) && w.causes.is_empty() {
                    w.causes.push(Cause {
                        generation: store.status.generation,
                        mutation: 0,
                        via: "quote".into(),
                    });
                }
            }
        }
        for (doc, record) in &store.docs {
            for (r, sec) in &record.sections {
                if section_has_body(sec) && !record.coverage.contains_key(r) {
                    let w = work.entry(format!("{}#{}", doc, r)).or_default();
                    w.unprocessed = true;
                    if w.causes.is_empty() {
                        w.causes.push(Cause {
                            generation: store.status.generation,
                            mutation: 0,
                            via: "section".into(),
                        });
                    }
                }
            }
        }
        let mut out = Vec::new();
        if work.is_empty() {
            return Vec::new();
        }
        let per_section = requirements_per_section(store);
        let linked = linked_subjects_all(store);
        let names = EntityNames::build(store);
        let mut doc_lines: BTreeMap<String, DocHintLines> = BTreeMap::new();
        for (full, w) in work {
            let Some((doc, sec_ref)) = split_section_ref(&full) else {
                continue;
            };
            let sec = &store.docs[&doc].sections[&sec_ref];
            let stale: Vec<String> = w.stale.iter().cloned().collect();
            let mut change = json!({});
            if let Some(d) = &w.dirty {
                change["dirty"] = d.clone();
            }
            if !stale.is_empty() {
                change["staleAnchors"] = json!(stale);
            }
            if w.unprocessed {
                change["unprocessed"] = json!(true);
            }
            let cause = w
                .causes
                .iter()
                .min_by_key(|c| (c.generation, c.mutation))
                .cloned();
            let sourced = per_section.get(&full).copied().unwrap_or(0);
            let mut hints = Vec::new();
            match (&w.dirty, w.unprocessed) {
                (Some(d), _) => {
                    let summary = ["added", "removed", "changed"]
                        .iter()
                        .filter_map(|k| d[k].as_u64().map(|n| format!("{} {}", n, k)))
                        .collect::<Vec<_>>();
                    hints.push(if summary.is_empty() {
                        format!(
                            "section dirty (edit g{})",
                            cause.as_ref().map(|c| c.generation).unwrap_or(0)
                        )
                    } else {
                        format!(
                            "{} (edit g{})",
                            summary.join(", "),
                            cause.as_ref().map(|c| c.generation).unwrap_or(0)
                        )
                    });
                }
                (None, true) => hints.push("unprocessed section".into()),
                _ => {}
            }
            hints.push(format!(
                "{} requirement(s) already sourced here; an unchanged statement is a no-op",
                sourced
            ));
            for a in &stale {
                let what = store
                    .graph
                    .requirements
                    .get(a)
                    .map(|r| {
                        format!(
                            "\"{}\" was quoted \"{}\"",
                            truncate(&r.statement, 80),
                            truncate(
                                r.source.as_ref().map(|s| s.quote.as_str()).unwrap_or(""),
                                60
                            )
                        )
                    })
                    .or_else(|| {
                        store
                            .graph
                            .entities
                            .get(a)
                            .map(|e| format!("entity {}: a mention's quote died", e.name))
                    })
                    .unwrap_or_default();
                hints.push(format!("stale {} {}", a, what));
            }
            // The document-level lines, once per document.
            // Mirrors docs/compiler/goals/reconcile-section.md#what-the-model-sees.
            let lines = doc_lines.entry(doc.clone()).or_insert_with(|| {
                doc_hint_lines(
                    store,
                    &doc,
                    linked.get(&doc).map(Vec::as_slice).unwrap_or(&[]),
                    &names,
                )
            });
            if let Some(l) = &lines.subjects {
                hints.push(l.clone());
            }
            hints.push(lines.known.clone());
            hints.push(format!("load {}", full));
            if sec.kind == "blob" {
                hints.push("Whole-file input: envelope metadata is not prose. Never quote it. Reconcile its role against incoming document context; a covered note names linked requirement IDs.".into());
                hints.push(format!(
                    "linked requirements: {}",
                    crate::input::requirement_links(store, &doc)
                        .into_iter()
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if sec.kind == "code" {
                hints.push(format!(
                    "code block: {} lines; coverage needs a requirement per behavioral step",
                    sec.raw.lines().count()
                ));
            }
            hints.push(
                "skill extraction; upsert_requirement, then set_coverage exactly once".into(),
            );
            out.push(build_goal(self, &full, true, change, cause, hints));
        }
        out
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        if board.tier_open(0) > 0 {
            return Ready::Blocked("place-anchors goals decide the anchors first (tier 0)".into());
        }
        let doc = split_section_ref(&goal.target)
            .map(|(d, _)| d)
            .unwrap_or_default();
        if board.alignment_pending.contains(&doc) {
            return Ready::Blocked(format!(
                "alignment pending: g:place-anchors:{} decides the anchors first",
                doc
            ));
        }
        if let Some(section) = board.blob_inputs.get(&doc) {
            for referring in section {
                if board.goals.iter().any(|g| {
                    g.kind == "reconcile-section"
                        && crate::model::split_section_ref(&g.target)
                            .is_some_and(|(d, _)| &d == referring)
                        && board.open(&g.id)
                }) {
                    return Ready::Blocked(format!(
                        "referring document {referring} reconciles before its whole-file input"
                    ));
                }
            }
        }
        if let Some(level) = board.level_waiting(&doc) {
            return Ready::Blocked(format!("level {} documents reconcile first", level));
        }
        Ready::Ready
    }
    fn pack(&self, store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        let mut docs: BTreeSet<String> = BTreeSet::new();
        for g in batch {
            lines.push(format!(
                "- {} full (section body with the diff marked)",
                g.target
            ));
            for a in str_list(&g.change["staleAnchors"]) {
                lines.push(format!("- {} full (stale anchor)", a));
            }
            if let Some((doc, _)) = split_section_ref(&g.target) {
                docs.insert(doc);
            }
        }
        for doc in docs {
            for (id, name, at) in linked_subjects(store, &doc) {
                lines.push(format!(
                    "- linked from {} introduced {} ({}) stub",
                    at, id, name
                ));
            }
            let mut mentioned: Vec<String> = store
                .graph
                .entities
                .iter()
                .filter(|(_, e)| e.mentions.iter().any(|m| m.doc == doc))
                .map(|(id, _)| id.clone())
                .collect();
            mentioned.truncate(40);
            for id in mentioned {
                lines.push(format!("- {} stub", id));
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "upsert_entity",
            "update_entity",
            "delete_entity",
            "upsert_requirement",
            "update_requirement",
            "delete_requirement",
            "upsert_criterion",
            "update_criterion",
            "delete_criterion",
            "classify_input",
            "set_coverage",
        ]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = Vec::new();
        const REJECTED_NOTES: [&str; 3] = [
            "it states a fact, not a requirement",
            "it describes content or appearance, not behavior",
            "it is not a requirement on the system",
        ];
        for op in staged {
            match op {
                Op::SetCoverage {
                    doc,
                    section,
                    state,
                    note,
                } => {
                    if crate::input::is_blob(store, doc, section) {
                        let linked = crate::input::staged_requirement_links(store, doc, staged);
                        let note = note.as_deref().unwrap_or("");
                        if note.trim().is_empty() {
                            out.push(Violation::new(
                                "note-required",
                                format!(
                                    "{doc}#{section}: explain the whole-file compilation decision"
                                ),
                            ));
                        }
                        if state == "covered"
                            && (linked.is_empty()
                                || !linked.iter().all(|id| {
                                    note.split(|c: char| {
                                        c.is_whitespace()
                                            || matches!(c, ',' | ';' | '`' | '(' | ')')
                                    })
                                    .any(|word| word.trim_end_matches('.') == id)
                                }))
                        {
                            let message = if linked.is_empty() {
                                format!("{doc}#{section}: no requirement quote selects this input. Repair an existing requirement in its referring section to quote the linked item, then mark coverage; naming an ID in the note alone is insufficient")
                            } else {
                                format!("{doc}#{section}: name each requirement whose quoted reference selects this input: {linked:?}")
                            };
                            out.push(Violation::new("uncovered-claim", message));
                        }
                        if state == "non-normative" && !linked.is_empty() {
                            out.push(Violation::new("referenced-input", format!("{doc}#{section}: requirements {linked:?} reference this input; reconcile it as covered")));
                        }
                        continue;
                    }
                    if state == "covered" {
                        // A criterion sourced from the section covers it too
                        // (docs/compiler/goals/reconcile-section.md#gate).
                        let crit_at = |s: &Option<crate::model::SourceRef>| {
                            s.as_ref()
                                .is_some_and(|s| s.doc == *doc && s.section == *section)
                        };
                        let sourced = store
                            .graph
                            .requirements
                            .values()
                            .any(|r| r.anchored_at(doc, section))
                            || store.graph.criteria.values().any(|c| crit_at(&c.source))
                            || staged.iter().any(|o| match o {
                                Op::CreateRequirement { requirement, .. } => {
                                    requirement.anchored_at(doc, section)
                                }
                                Op::UpdateRequirement {
                                    source: Some(s), ..
                                } => s.doc == *doc && s.section == *section,
                                Op::CreateCriterion { criterion, .. } => crit_at(&criterion.source),
                                _ => false,
                            });
                        if !sourced {
                            out.push(Violation::new(
                                "uncovered-claim",
                                format!(
                                    "{}#{} is claimed covered but no requirement is sourced from it",
                                    doc, section
                                ),
                            ));
                        }
                        let missing = unaccounted_added_lines(
                            store,
                            staged,
                            doc,
                            section,
                            note.as_deref().unwrap_or(""),
                        );
                        if !missing.is_empty() {
                            out.push(Violation::new(
                                "added-line-unaccounted",
                                format!(
                                    "{}#{} is claimed covered but the edit added {} that no quote staged in this session carries: {}. The requirements recorded before the edit never carry an added line. Extract what each states, or repeat the line in the coverage note and say why it states nothing.",
                                    doc,
                                    section,
                                    if missing.len() == 1 { "a line" } else { "lines" },
                                    missing
                                        .iter()
                                        .map(|l| format!("\"{}\"", truncate(l, 160)))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
                            ));
                        }
                    } else if state == "non-normative" {
                        let n = note.as_deref().unwrap_or("").trim().to_lowercase();
                        if n.is_empty() {
                            out.push(Violation::new(
                                "note-required",
                                format!("{}#{} is non-normative without a note", doc, section),
                            ));
                        } else if REJECTED_NOTES.iter().any(|r| n.contains(r)) {
                            out.push(Violation::new(
                                "rejected-reason",
                                format!(
                                    "{}#{}: \"{}\" is one of the rejected non-normative reasons",
                                    doc, section, n
                                ),
                            ));
                        }
                    }
                }
                Op::CreateRequirement { id, requirement } => {
                    if let Some(s) = requirement.source.as_ref() {
                        if !store.quote_locates(&s.doc, &s.section, &s.quote) {
                            out.push(Violation::new(
                                "quote-not-found",
                                format!(
                                    "{}: the quote does not locate in {}#{}",
                                    id, s.doc, s.section
                                ),
                            ));
                        }
                    }
                }
                Op::UpdateRequirement {
                    id,
                    source: Some(s),
                    ..
                } if !store.quote_locates(&s.doc, &s.section, &s.quote) => {
                    out.push(Violation::new(
                        "quote-not-found",
                        format!(
                            "{}: the quote does not locate in {}#{}",
                            id, s.doc, s.section
                        ),
                    ));
                }
                _ => {}
            }
        }
        out
    }
    // The goal's stale anchors are addressed, its sections carry a coverage mark, and
    // every covered claim is honest. Mirrors docs/compiler/goals/reconcile-section.md#gate.
    fn goal_gate(&self, cx: &GateContext) -> Option<Violation> {
        let untouched = untouched_stale(cx.store, cx.staged, &cx.goal.stale_anchors);
        if !untouched.is_empty() {
            return Some(Violation::new(
                "stale-anchor",
                format!(
                    "{}: stale anchors left untouched: {}; re-record, revise, or delete each",
                    cx.goal.goal,
                    untouched.join(", ")
                ),
            ));
        }
        if let Some(doc) = &cx.goal.doc {
            let unmarked = unmarked_sections(cx.store, cx.staged, doc, &cx.goal.sections);
            if !unmarked.is_empty() {
                return Some(Violation::new(
                    "unmarked-section",
                    format!(
                        "{}: section(s) without a coverage mark: {}; set_coverage covered (a requirement must be sourced from it) or non-normative with a note",
                        cx.goal.goal,
                        unmarked.join(", ")
                    ),
                ));
            }
        }
        dishonest_covered(cx.store, cx.staged)
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../../docs/compiler/goals/prompts/reconcile-section.md")
    }
}
