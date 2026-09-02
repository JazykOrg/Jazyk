// The reconciler: compares the documents (desired state) against the graph (observed
// state), derives the goal board, and runs sessions one batch at a time until no goal
// is ready or the budget is spent. Then the deterministic tail: the checks, flip
// detection, rendering, docsgen, the verdict. Deterministic; the model never decides
// what is stale or what runs next. Mirrors docs/compiler/reconciler.md and
// docs/compiler/compilation.md.
use crate::board::Board;
use crate::control::Control;
use crate::goals;
use crate::llm::Llm;
use crate::md;
use crate::model::*;
use crate::project::Project;
use crate::session::{Trace, TraceEvent};
use crate::store::Store;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildReport {
    pub verdict: String,
    // Goals the board derived when the build opened.
    pub goals: usize,
    pub sessions: u32,
    pub applied: usize,
    pub parked: usize,
    pub failed: usize,
    pub blocked: usize,
    pub optional: usize,
    pub errors: usize,
    pub warnings: usize,
    pub coverage_pct: u32,
    pub tokens: u64,
}

impl BuildReport {
    pub fn converged(&self) -> bool {
        self.verdict.starts_with("converged")
    }
}

// One deterministic finding: rule, subjects, severity, message, and the question the
// finding carries when its resolution is enumerable. Most checks name one subject;
// nondeterministic-transition names the pair.
pub type Finding = (
    String,
    Vec<String>,
    String,
    String,
    Option<DiagnosticPrompt>,
);

fn finding(rule: &str, subject: &str, severity: &str, message: String) -> Finding {
    (
        rule.to_string(),
        vec![subject.to_string()],
        severity.to_string(),
        message,
        None,
    )
}

// Parse every matched document. Returns (doc -> (content hash, sections), doc -> links).
pub fn parse_all(
    proj: &Project,
) -> (
    BTreeMap<String, (String, BTreeMap<String, Section>)>,
    BTreeMap<String, Vec<String>>,
) {
    let mut parsed = BTreeMap::new();
    let mut links = BTreeMap::new();
    for f in proj.doc_files() {
        let rel = match f.strip_prefix(&proj.root) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => f.to_string_lossy().replace('\\', "/"),
        };
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        links.insert(rel.clone(), md::doc_links(&text, &rel));
        parsed.insert(rel, (hash_hex(&text), md::parse_sections(&text)));
    }
    (parsed, links)
}

// A non-normative claim over text that still reads as obligations. Deliberately cheap
// and deterministic: `shall`, obligation verbs, access rules, or definition-list
// bullets (`- \`name\` - description`). Docs rarely say `shall`, so the word alone
// misses whole documents. Mirrors docs/compiler/compilation.md#coverage.
fn looks_normative(raw: &str) -> bool {
    // A lead-in-only body (one sentence ending in a colon, its items living in child
    // sections) states nothing by itself; non-normative is the correct mark there.
    let body: Vec<&str> = raw
        .lines()
        .enumerate()
        .filter(|(i, l)| !(*i == 0 && l.trim_start().starts_with('#')))
        .map(|(_, l)| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if let [only] = body.as_slice() {
        if only.ends_with(':') && !only.starts_with('-') {
            return false;
        }
    }
    let t = raw.to_lowercase();
    const SIGNALS: [&str; 10] = [
        " shall ",
        " supports ",
        " manages ",
        " handles ",
        " provides ",
        " requires ",
        " allows ",
        " stores ",
        " can be performed ",
        " is responsible ",
    ];
    if SIGNALS.iter().any(|s| t.contains(s)) {
        return true;
    }
    raw.lines().any(|l| {
        let l = l.trim_start();
        l.starts_with("- `") && l[3..].contains("` - ")
    })
}

// Pinned-fact drift: a code-span literal in a bound requirement's statement that
// none of its bound files mention. The docs say `us-east-1` and the code never does:
// one of them is wrong, and no model is needed to notice. The finding carries its
// question, and an answered question is never re-asked (the prompt merge in
// reconcile_check_diags). Mirrors docs/compiler/compilation.md#checks.
fn drift_checks(store: &Store, proj: &Project) -> Vec<Finding> {
    let ledger = crate::gen::Ledger::load(&proj.out);
    if ledger.requirements.is_empty() {
        return Vec::new();
    }
    let gs = crate::gen::GenSettings::resolve(proj);
    let mut findings = Vec::new();
    for (rid, row) in &ledger.requirements {
        let Some(req) = store.graph.requirements.get(rid) else {
            continue;
        };
        let mut files: Vec<String> = row.files.clone();
        if let Some(e) = ledger.entities.get(&row.entity) {
            files.extend(e.files.iter().cloned());
        }
        files.sort();
        files.dedup();
        if files.is_empty() {
            continue;
        }
        let mut contents = String::new();
        for f in &files {
            if let Ok(t) = std::fs::read_to_string(gs.deliverable.join(f)) {
                contents.push_str(&t);
                contents.push('\n');
            }
        }
        if contents.is_empty() {
            continue;
        }
        for lit in pinned_literals(&req.statement) {
            if contents.contains(&lit) {
                continue;
            }
            let prompt = DiagnosticPrompt {
                question: format!("The docs pin `{}` but none of the bound files mention it. Which is right?", lit),
                options: vec![
                    PromptOption {
                        label: "The docs are right; the code must change".into(),
                        edit: None,
                        answer: Some(format!(
                            "The documents are correct: make the implementation use `{}` and rerun verification.",
                            lit
                        )),
                    },
                    PromptOption {
                        label: "This value is not pinned for these files".into(),
                        edit: None,
                        answer: Some(format!(
                            "`{}` is context, not a pinned fact for these files; reword or retarget the requirement so the check stops matching it.",
                            lit
                        )),
                    },
                ],
                freeform: true,
            };
            findings.push((
                "pinned-fact-drift".to_string(),
                vec![rid.clone()],
                "warning".to_string(),
                format!(
                    "{} pins `{}`; bound file(s) {} never mention it",
                    rid,
                    lit,
                    files.join(", ")
                ),
                Some(prompt),
            ));
            // One finding per requirement keeps the noise bounded.
            break;
        }
    }
    findings
}

// A code-span token that reads as a pinned fact: one word, long enough to be a
// value, carrying a digit, dot, slash, dash, colon, or underscore.
fn pinned_literals(statement: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut rest = statement;
    while let Some(b) = rest.find('`') {
        let after = &rest[b + 1..];
        let Some(e) = after.find('`') else { break };
        let tok = after[..e].trim();
        rest = &after[e + 1..];
        if tok.len() >= 4
            && tok.len() <= 80
            && !tok.contains(' ')
            && tok
                .chars()
                .any(|c| c.is_ascii_digit() || ['.', '/', '-', ':', '_'].contains(&c))
        {
            v.push(tok.to_string());
        }
    }
    v.sort();
    v.dedup();
    v
}

fn node_alive(store: &Store, id: &str) -> bool {
    let r = store.resolve_id(id);
    store.graph.entities.contains_key(r)
        || store.graph.requirements.contains_key(r)
        || store.graph.views.contains_key(r)
}

fn section_alive(store: &Store, doc: &str, section: &str) -> bool {
    store
        .docs
        .get(doc)
        .is_some_and(|r| r.sections.contains_key(section))
}

fn open_proposal_on(store: &Store, subject: &str) -> bool {
    store.graph.diagnostics.values().any(|d| {
        d.lifecycle == "open"
            && d.rule == "ratification-pending"
            && d.subjects.iter().any(|s| s == subject)
    })
}

// Whether a provenance justifies its fact: a quote in a live section, or a derived or
// decree provenance with live upstream nodes and an open ratification proposal.
// Mirrors docs/compiler/compilation.md#checks (justification closure).
fn justified(store: &Store, subject: &str, p: &Provenance) -> Result<(), String> {
    match p {
        Provenance::Quote(s) => {
            if section_alive(store, &s.doc, &s.section) {
                Ok(())
            } else {
                Err(format!(
                    "its quote names {}#{}, which no longer exists",
                    s.doc, s.section
                ))
            }
        }
        Provenance::Derived { from, .. } => {
            let dead: Vec<&String> = from.iter().filter(|f| !node_alive(store, f)).collect();
            if !dead.is_empty() {
                return Err(format!(
                    "it is derived from {}, which no longer exist",
                    dead.iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if open_proposal_on(store, subject) {
                Ok(())
            } else {
                Err("it is derived and carries no open ratification proposal".into())
            }
        }
        Provenance::Decree { .. } => {
            if open_proposal_on(store, subject) {
                Ok(())
            } else {
                Err("it is a decree and carries no open ratification proposal".into())
            }
        }
    }
}

// The flow view a requirement fits best: its own cluster's view, else the flow view
// sharing the most entities (same document first, then the first member's order,
// then id). Returns the ranking, best first. Mirrors docs/compiler/goals/curate-view.md.
fn flow_placement_ranking(store: &Store, rid: &str) -> Vec<String> {
    let Some(r) = store.graph.requirements.get(rid) else {
        return Vec::new();
    };
    let ents: BTreeSet<String> = r
        .entities
        .iter()
        .map(|e| store.resolve_id(e).to_string())
        .collect();
    let doc = r.source.as_ref().map(|s| s.doc.clone()).unwrap_or_default();
    let order = crate::derive::document_order(store);
    let position = |id: &str| order.iter().position(|x| x == id).unwrap_or(usize::MAX);
    let kind_rank = |k: &str| goals::FLOW_KINDS.iter().position(|x| *x == k).unwrap_or(9);
    let mut ranked: Vec<(std::cmp::Reverse<usize>, bool, usize, usize, String)> = store
        .graph
        .views
        .iter()
        .filter(|(_, v)| goals::is_flow_kind(&v.kind))
        .map(|(vid, v)| {
            let members: Vec<&Requirement> = v
                .members
                .iter()
                .filter_map(|m| store.graph.requirements.get(m))
                .collect();
            let shared = members
                .iter()
                .flat_map(|m| m.entities.iter().map(|e| store.resolve_id(e)))
                .collect::<BTreeSet<_>>()
                .iter()
                .filter(|e| ents.contains(**e))
                .count();
            let same_doc = members
                .iter()
                .any(|m| m.source.as_ref().is_some_and(|s| s.doc == doc));
            let first = v.members.first().map(|m| position(m)).unwrap_or(usize::MAX);
            (
                std::cmp::Reverse(shared),
                !same_doc,
                kind_rank(&v.kind),
                first,
                vid.clone(),
            )
        })
        .collect();
    ranked.sort();
    let mut ids: Vec<String> = ranked.into_iter().map(|(_, _, _, _, id)| id).collect();
    // The requirement's own cluster (actor and document) outranks every other.
    if let Some(actor) = flow_actor(store, r) {
        let stem = doc
            .rsplit('/')
            .next()
            .unwrap_or(&doc)
            .trim_end_matches(".md");
        let own = format!(
            "view:usecase/{}-{}",
            crate::derive::entity_slug(&actor),
            md::slug(stem)
        );
        if let Some(pos) = ids.iter().position(|v| *v == own) {
            let v = ids.remove(pos);
            ids.insert(0, v);
        }
    }
    ids
}

fn flow_actor(store: &Store, r: &Requirement) -> Option<String> {
    let resolved: Vec<String> = r
        .entities
        .iter()
        .map(|e| store.resolve_id(e).to_string())
        .filter(|e| store.graph.entities.contains_key(e))
        .collect();
    resolved
        .iter()
        .find(|e| {
            store.graph.entities[*e]
                .stereotype
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("actor"))
        })
        .or_else(|| resolved.first())
        .cloned()
}

// The flow placement findings, each with the view the record lands on and the
// alternatives. Runs only where flow views exist.
fn flow_placement(store: &Store) -> Vec<(Finding, Option<String>, Vec<String>, String)> {
    if !store
        .graph
        .views
        .values()
        .any(|v| goals::is_flow_kind(&v.kind))
    {
        return Vec::new();
    }
    let placed: BTreeSet<&str> = store
        .graph
        .views
        .values()
        .filter(|v| goals::is_flow_kind(&v.kind))
        .flat_map(|v| v.members.iter().map(String::as_str))
        .collect();
    let excluded: BTreeSet<&str> = store
        .graph
        .views
        .values()
        .filter(|v| goals::is_flow_kind(&v.kind))
        .flat_map(|v| v.excluded.iter())
        .filter(|x| !x.note.trim().is_empty())
        .map(|x| x.id.as_str())
        .collect();
    let mut out = Vec::new();
    for (rid, r) in &store.graph.requirements {
        let facet = r
            .facets
            .iter()
            .map(|f| f.facet.as_str())
            .find(|f| *f == "behavior" || *f == "failure-mode");
        let Some(facet) = facet else { continue };
        if placed.contains(rid.as_str()) || excluded.contains(rid.as_str()) {
            continue;
        }
        let ranking = flow_placement_ranking(store, rid);
        let (rule, message) = if facet == "behavior" {
            (
                "unplaced-behavior",
                format!(
                    "{} is a behavior requirement in no flow view and excluded from none",
                    rid
                ),
            )
        } else {
            (
                "unrepresented-failure-mode",
                format!(
                    "{} is a failure-mode requirement no flow branch represents",
                    rid
                ),
            )
        };
        out.push((
            finding(rule, rid, "info", message),
            ranking.first().cloned(),
            ranking.iter().skip(1).take(3).cloned().collect(),
            facet.to_string(),
        ));
    }
    out
}

// The deterministic checks over the whole graph. Mirrors docs/compiler/compilation.md#checks.
pub fn checks(store: &Store, proj: &Project) -> Vec<Finding> {
    let mut f: Vec<Finding> = Vec::new();
    // File-level document quality: an empty file schedules no session and a link only
    // feeds levels, so neither problem ever reaches a model.
    for (doc, rec) in &store.docs {
        let no_content = rec.sections.values().all(|sec| {
            let skip = if sec.kind == "heading" { 1 } else { 0 };
            sec.raw.lines().skip(skip).all(|l| l.trim().is_empty())
        });
        if no_content {
            f.push(finding(
                "empty-file",
                doc,
                "warning",
                format!("{} is matched by the docs glob but has no content", doc),
            ));
        }
        let mut reported: BTreeSet<String> = BTreeSet::new();
        for sec in rec.sections.values() {
            for target in md::doc_links(&sec.raw, doc) {
                if store.docs.contains_key(&target)
                    || proj.root.join(&target).exists()
                    || !reported.insert(target.clone())
                {
                    continue;
                }
                f.push(finding(
                    "broken-link",
                    doc,
                    "warning",
                    format!("{} links to {} which does not exist", doc, target),
                ));
            }
        }
    }
    // Coverage: sections with a body of their own that stayed unprocessed, and
    // non-normative marks over text that still looks normative.
    for (doc, rec) in &store.docs {
        for (r, sec) in &rec.sections {
            if !goals::section_has_body(sec) {
                continue;
            }
            match rec.coverage.get(r) {
                None => f.push(finding(
                    "uncovered-section",
                    &format!("{}#{}", doc, r),
                    "warning",
                    format!("section {}#{} is unprocessed after the build", doc, r),
                )),
                Some(c) if c.state == "non-normative" && looks_normative(&sec.raw) => {
                    f.push(finding(
                        "suspicious-non-normative",
                        &format!("{}#{}", doc, r),
                        "warning",
                        format!(
                        "section {}#{} is marked non-normative but its text still looks normative",
                        doc, r
                    ),
                    ))
                }
                _ => {}
            }
        }
    }
    // Entities no requirement references.
    for id in store.graph.entities.keys() {
        if store.requirements_referencing(id).is_empty() {
            f.push(finding(
                "unused-entity",
                id,
                "warning",
                format!("{} has no requirement referencing it", id),
            ));
        }
    }
    // Reachability from root entities (entities mentioned in a root document).
    let root_entities: BTreeSet<String> = store
        .graph
        .entities
        .iter()
        .filter(|(_, e)| e.mentions.iter().any(|m| proj.is_root_file(&m.doc)))
        .map(|(id, _)| id.clone())
        .collect();
    if !root_entities.is_empty() {
        let mut reach = root_entities.clone();
        let mut frontier: Vec<String> = root_entities.into_iter().collect();
        while let Some(id) = frontier.pop() {
            for rel in store.graph.relationships.values() {
                if rel.members.contains(&id) {
                    for m in &rel.members {
                        if reach.insert(m.clone()) {
                            frontier.push(m.clone());
                        }
                    }
                }
            }
            for r in store.graph.requirements.values() {
                if r.entities.contains(&id) {
                    for m in &r.entities {
                        if reach.insert(m.clone()) {
                            frontier.push(m.clone());
                        }
                    }
                }
            }
        }
        for id in store.graph.entities.keys() {
            if !reach.contains(id) {
                f.push(finding(
                    "unreachable-entity",
                    id,
                    "warning",
                    format!("{} is not reachable from the declared roots", id),
                ));
            }
        }
    }
    // Document quality: prose problems a human can fix, surfaced where the human writes.
    for (doc, rec) in &store.docs {
        if rec.sections.len() > crate::limits::MAX_DOC_SECTIONS {
            let root = rec
                .sections
                .iter()
                .find(|(_, s)| s.kind == "root")
                .map(|(r, _)| r.clone())
                .unwrap_or_default();
            f.push(finding(
                "doc-too-large",
                &format!("{}#{}", doc, root),
                "warning",
                format!(
                    "{} has {} sections (cap {}); split the document",
                    doc,
                    rec.sections.len(),
                    crate::limits::MAX_DOC_SECTIONS
                ),
            ));
        }
        for (r, sec) in &rec.sections {
            if sec.raw.len() > crate::limits::MAX_SECTION_CHARS {
                f.push(finding(
                    "section-too-large",
                    &format!("{}#{}", doc, r),
                    "warning",
                    format!(
                        "{}#{} is {} chars (cap {}); split the section",
                        doc,
                        r,
                        sec.raw.len(),
                        crate::limits::MAX_SECTION_CHARS
                    ),
                ));
            }
        }
    }
    // Near-identical statements on one entity: review debt made deterministic.
    {
        let toks = |s: &str| -> BTreeSet<String> {
            s.to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .filter(|t| t.len() > 2)
                .map(String::from)
                .collect()
        };
        let mut flagged: BTreeSet<(String, String)> = BTreeSet::new();
        let mut by_entity: BTreeMap<&String, Vec<(&String, BTreeSet<String>)>> = BTreeMap::new();
        for (rid, r) in &store.graph.requirements {
            for e in &r.entities {
                by_entity
                    .entry(e)
                    .or_default()
                    .push((rid, toks(&r.statement)));
            }
        }
        let norm = crate::store::normalize_statement;
        for list in by_entity.values() {
            for i in 0..list.len() {
                for j in i + 1..list.len() {
                    let (a, ta) = &list[i];
                    let (b, tb) = &list[j];
                    let inter = ta.intersection(tb).count();
                    let union = ta.union(tb).count();
                    if union == 0 || inter * 10 < union * 8 {
                        continue;
                    }
                    let (ra, rb) = (&store.graph.requirements[*a], &store.graph.requirements[*b]);
                    let key = if a < b {
                        ((*a).clone(), (*b).clone())
                    } else {
                        ((*b).clone(), (*a).clone())
                    };
                    if !flagged.insert(key.clone()) {
                        continue;
                    }
                    let (Some(sa), Some(sb)) = (ra.source.as_ref(), rb.source.as_ref()) else {
                        continue;
                    };
                    if sa.doc == sb.doc {
                        if sa.section == sb.section && norm(&sa.quote) == norm(&sb.quote) {
                            f.push(finding(
                                "duplicate-requirement",
                                &key.0,
                                "warning",
                                format!(
                                    "{} and {} extract the same sentence twice; keep one",
                                    key.0, key.1
                                ),
                            ));
                        }
                    } else {
                        f.push(finding(
                            "duplicate-requirement",
                            &key.0,
                            "info",
                            format!(
                                "{} and {} state the same fact in different documents; both kept",
                                key.0, key.1
                            ),
                        ));
                    }
                }
            }
        }
    }
    // Unstable extraction: an entity id minted with a collision suffix while a
    // tombstone holds the base slug means a natural key was deleted and recreated.
    for id in store.graph.entities.keys() {
        if let Some(pos) = id.rfind('-') {
            let (base, suffix) = id.split_at(pos);
            if suffix[1..].chars().all(|c| c.is_ascii_digit())
                && store
                    .graph
                    .redirects
                    .get(base)
                    .map(|t| t.is_empty())
                    .unwrap_or(false)
            {
                f.push(finding(
                    "unstable-extraction",
                    id,
                    "warning",
                    format!(
                        "{} recreates a natural key that was deleted in an earlier build ({})",
                        id, base
                    ),
                ));
            }
        }
    }
    // Quotes that no longer locate.
    for (rid, r) in &store.graph.requirements {
        let Some(src) = r.source.as_ref() else {
            continue;
        };
        if section_alive(store, &src.doc, &src.section)
            && !store.quote_locates(&src.doc, &src.section, &src.quote)
        {
            f.push(finding(
                "stale-provenance",
                rid,
                "warning",
                format!(
                    "{}'s quote no longer locates in {}#{}",
                    rid, src.doc, src.section
                ),
            ));
        }
    }
    // Justification closure: every fact walks to a quote in a live section, or to a
    // derived or decree provenance with live upstream nodes and an open proposal.
    for (rid, r) in &store.graph.requirements {
        let why = match r.provenance() {
            None => Some("it carries no provenance".to_string()),
            Some(ProvenanceRef::Quote(s)) => {
                justified(store, rid, &Provenance::Quote(s.clone())).err()
            }
            Some(_) => r
                .provenance
                .as_ref()
                .and_then(|p| justified(store, rid, p).err()),
        };
        if let Some(why) = why {
            f.push(finding(
                "unjustified-fact",
                rid,
                "error",
                format!("{} is unjustified: {}", rid, why),
            ));
        }
    }
    for (id, e) in &store.graph.entities {
        let live_mention = e
            .mentions
            .iter()
            .any(|m| section_alive(store, &m.doc, &m.section));
        let why = if live_mention {
            None
        } else if let Some(p) = &e.provenance {
            justified(store, id, p).err()
        } else if e.mentions.is_empty() {
            Some("it has no mention and no provenance".to_string())
        } else {
            Some("every section that mentioned it is gone".to_string())
        };
        if let Some(why) = why {
            f.push(finding(
                "unjustified-fact",
                id,
                "error",
                format!("{} is unjustified: {}", id, why),
            ));
        }
        for a in &e.attributes {
            let why = match &a.provenance {
                Provenance::Quote(s) => (!section_alive(store, &s.doc, &s.section)).then(|| {
                    format!(
                        "attribute {} quotes {}#{}, which no longer exists",
                        a.name, s.doc, s.section
                    )
                }),
                Provenance::Derived { from, .. } => {
                    let dead: Vec<&String> =
                        from.iter().filter(|x| !node_alive(store, x)).collect();
                    (!dead.is_empty()).then(|| {
                        format!(
                            "attribute {} is derived from {}, which no longer exist",
                            a.name,
                            dead.iter()
                                .map(|s| s.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    })
                }
                Provenance::Decree { .. } => None,
            };
            if let Some(why) = why {
                f.push(finding(
                    "unjustified-fact",
                    id,
                    "error",
                    format!("{} is unjustified: {}", id, why),
                ));
                break;
            }
        }
    }
    for (vid, v) in &store.graph.views {
        if v.default {
            continue;
        }
        if let Some(Provenance::Derived { from, .. }) = &v.provenance {
            let dead: Vec<&String> = from
                .iter()
                .filter(|x| !node_alive(store, x) && !store.graph.views.contains_key(*x))
                .collect();
            if !dead.is_empty() {
                f.push(finding(
                    "unjustified-fact",
                    vid,
                    "error",
                    format!(
                        "{} is unjustified: it is derived from {}, which no longer exist",
                        vid,
                        dead.iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ));
            }
        }
    }
    // Flow placement, wherever flow views exist.
    for (finding, _, _, _) in flow_placement(store) {
        f.push(finding);
    }
    // Containment consistency: a composition never crosses the tree sideways.
    for rel in store.graph.relationships.values() {
        for c in &rel.contributions {
            if c.r#type != "composition" {
                continue;
            }
            let (whole, part) = (&c.a, &c.b);
            let Some(parent) = store
                .graph
                .entities
                .get(part)
                .and_then(|e| e.parent.clone())
            else {
                continue;
            };
            let comparable = parent == *whole
                || store.is_ancestor(whole, &parent)
                || store.is_ancestor(&parent, whole);
            if !comparable {
                f.push(finding(
                    "containment-mismatch",
                    part,
                    "warning",
                    format!(
                        "{} is composed into {} but its parent {} sits in another branch of the tree",
                        part, whole, parent
                    ),
                ));
            }
        }
    }
    // Conformance, the mechanical part: an instance's attribute names against its type.
    for (inst, ty) in crate::derive::instance_types(store) {
        let Some(e) = store.graph.entities.get(&inst) else {
            continue;
        };
        if !store.graph.entities.contains_key(&ty) {
            continue;
        }
        let declared = goals::declared_attributes(store, &ty);
        let stray: Vec<&str> = e
            .attributes
            .iter()
            .filter(|a| !declared.contains(&a.name.to_lowercase()))
            .map(|a| a.name.as_str())
            .collect();
        if !stray.is_empty() {
            f.push(finding(
                "nonconformant-instance",
                &inst,
                "warning",
                format!(
                    "{} carries attribute(s) {} that its type {} does not declare",
                    inst,
                    stray.join(", "),
                    ty
                ),
            ));
        }
    }
    // State machine checks.
    for m in store.graph.state_machines.values() {
        f.extend(machine_checks(m));
    }
    // Provider check, wherever interface-like entities exist. Interface-like keys on
    // structure, not the label: something realizes the entity, or the `interface`
    // stereotype marks it before anything does. Mirrors docs/compiler/model/entity.md#fields.
    for (iface, e) in &store.graph.entities {
        let contributions: Vec<&Contribution> = store
            .graph
            .relationships
            .values()
            .flat_map(|r| r.contributions.iter())
            .filter(|c| &c.b == iface)
            .collect();
        let realizers: Vec<&str> = contributions
            .iter()
            .filter(|c| c.r#type == "realization")
            .map(|c| c.a.as_str())
            .collect();
        let interface_like = !realizers.is_empty()
            || e.stereotype
                .as_deref()
                .is_some_and(|s| s.to_lowercase().contains("interface"));
        if !interface_like {
            continue;
        }
        let dependents = contributions
            .iter()
            .filter(|c| c.r#type == "dependency")
            .count();
        if dependents == 0 {
            continue;
        }
        match realizers.len() {
            0 => f.push(finding(
                "provider-missing",
                iface,
                "warning",
                format!("{} is depended on but nothing realizes it", iface),
            )),
            1 => {}
            _ => f.push(finding(
                "provider-ambiguous",
                iface,
                "warning",
                format!(
                    "{} has more than one realizer: {}",
                    iface,
                    realizers.join(", ")
                ),
            )),
        }
    }
    // A quality facet without a measure.
    for (rid, r) in &store.graph.requirements {
        if r.facets
            .iter()
            .any(|x| x.facet == "quality" && x.measure.as_deref().unwrap_or("").trim().is_empty())
        {
            f.push(finding(
                "quality-unmeasured",
                rid,
                "warning",
                format!("{} states a quality without a measurable bound", rid),
            ));
        }
    }
    // Decompiled documents nobody has touched since the machine wrote them.
    for doc in crate::decompile::unratified(store) {
        f.push(finding(
            "unratified",
            &doc,
            "info",
            format!("{} is a decompiled draft nobody has ratified; review it by editing it, even a one-line change clears this", doc),
        ));
    }
    // Goals parked when a budget ran out.
    for g in &store.status.parked {
        f.push(finding(
            "incomplete-build",
            &g.target,
            "warning",
            format!("{} was parked; the next build resumes it", g.id),
        ));
    }
    f.extend(drift_checks(store, proj));
    f
}

// The state machine checks. Mirrors docs/compiler/model/state-machine.md#checks.
fn machine_checks(m: &StateMachine) -> Vec<Finding> {
    let mut f = Vec::new();
    let norm = crate::derive::normalize_state;
    let states: Vec<String> = m.states.iter().map(|s| norm(s)).collect();
    let entered: BTreeSet<String> = m.transitions.iter().map(|t| norm(&t.to)).collect();
    let initial: Vec<String> = states
        .iter()
        .filter(|s| !entered.contains(*s))
        .cloned()
        .collect();
    if !initial.is_empty() {
        let mut reach: BTreeSet<String> = initial.iter().cloned().collect();
        let mut frontier: Vec<String> = initial.clone();
        while let Some(s) = frontier.pop() {
            for t in &m.transitions {
                if norm(&t.from) == s && reach.insert(norm(&t.to)) {
                    frontier.push(norm(&t.to));
                }
            }
        }
        let unreachable: Vec<&String> = states.iter().filter(|s| !reach.contains(*s)).collect();
        if !unreachable.is_empty() {
            f.push(finding(
                "unreachable-state",
                &m.subject,
                "warning",
                format!(
                    "{}: no path from the initial state reaches {}",
                    m.subject,
                    unreachable
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
    }
    let leaving: BTreeSet<String> = m.transitions.iter().map(|t| norm(&t.from)).collect();
    let dead: Vec<&String> = states.iter().filter(|s| !leaving.contains(*s)).collect();
    if !dead.is_empty() {
        f.push(finding(
            "dead-end-state",
            &m.subject,
            "info",
            format!(
                "{}: {} has no outgoing transition (the final state, or a requirements gap)",
                m.subject,
                dead.iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    for i in 0..m.transitions.len() {
        for j in i + 1..m.transitions.len() {
            let (a, b) = (&m.transitions[i], &m.transitions[j]);
            if norm(&a.from) != norm(&b.from) || a.trigger != b.trigger || a.trigger.is_none() {
                continue;
            }
            let overlap = match (&a.guard, &b.guard) {
                (None, _) | (_, None) => true,
                (Some(x), Some(y)) => norm(x) == norm(y),
            };
            if overlap && norm(&a.to) != norm(&b.to) {
                // One diagnostic per pair, the two requirements as subjects, so deleting
                // either one re-triggers rejudge through deletion propagation.
                f.push((
                    "nondeterministic-transition".to_string(),
                    vec![a.requirement.clone(), b.requirement.clone()],
                    "warning".to_string(),
                    format!(
                        "{} and {} both leave {} on {} with overlapping guards",
                        a.requirement,
                        b.requirement,
                        a.from,
                        a.trigger.as_deref().unwrap_or("")
                    ),
                    None,
                ));
            }
        }
    }
    // A single-transition machine makes every other state a trivially unhandled dead
    // end that dead-end-state already reports on the same build: unhandled-event
    // speaks only once the machine has at least two transitions.
    let triggers: BTreeSet<String> = if m.transitions.len() >= 2 {
        m.transitions
            .iter()
            .filter_map(|t| t.trigger.clone())
            .collect()
    } else {
        BTreeSet::new()
    };
    let mut unhandled: Vec<String> = Vec::new();
    for s in &states {
        // A state with no outgoing transition is dead-end-state's finding; enumerating
        // its triggers here would only restate it as a cross-product wall.
        // Mirrors docs/compiler/model/state-machine.md#checks.
        if !leaving.contains(s) {
            continue;
        }
        for t in &triggers {
            let handled = m
                .transitions
                .iter()
                .any(|x| norm(&x.from) == *s && x.trigger.as_deref() == Some(t.as_str()));
            if !handled {
                unhandled.push(format!("{} on {}", s, t));
            }
        }
    }
    if !unhandled.is_empty() {
        f.push(finding(
            "unhandled-event",
            &m.subject,
            "info",
            format!("{}: no transition for {}", m.subject, unhandled.join("; ")),
        ));
    }
    f
}

// Cross-class flip detection over the journal: a natural key a GC commit and a compile
// commit hand back and forth. Two flips park the pair and file one
// `unstable-derivation` diagnostic carrying both justifications.
// Mirrors docs/compiler/reconciler.md#flip-detection.
pub fn flip_detection(store: &Store) -> (Vec<Finding>, Vec<Goal>) {
    struct Event {
        generation: u64,
        class: goals::Class,
        goal: String,
        justification: String,
    }
    let entries = goals::read_journal(&store.out);
    let key_of_entity =
        |name: &str, scope: &str| format!("{}|{}", crate::store::normalize_statement(name), scope);
    let mut id_key: BTreeMap<String, String> = store
        .graph
        .entities
        .iter()
        .map(|(id, e)| (id.clone(), key_of_entity(&e.name, &e.scope)))
        .collect();
    let mut history: BTreeMap<String, Vec<Event>> = BTreeMap::new();
    for entry in &entries {
        if entry.kind != "session" {
            continue;
        }
        let goal_of_class = |class: goals::Class| -> Option<String> {
            entry
                .batch
                .iter()
                .find(|g| {
                    goals::parse_goal_id(g)
                        .and_then(|(k, _)| goals::kind(k))
                        .is_some_and(|k| k.class() == class)
                })
                .cloned()
        };
        let class = if goal_of_class(goals::Class::Gc).is_some() {
            goals::Class::Gc
        } else {
            goals::Class::Compile
        };
        let Some(goal) = goal_of_class(class) else {
            continue;
        };
        let justification = entry
            .resolved_goals
            .iter()
            .find(|r| r.goal == goal)
            .map(|r| r.justification.clone())
            .unwrap_or_default();
        for m in &entry.mutations {
            let Some(o) = m.as_object() else { continue };
            for (op, body) in o {
                let key = match op.as_str() {
                    "CreateEntity" => {
                        let id = body["id"].as_str().unwrap_or("").to_string();
                        let name = body["entity"]["name"].as_str().unwrap_or("");
                        let scope = body["entity"]["scope"].as_str().unwrap_or("public");
                        let key = key_of_entity(name, scope);
                        id_key.insert(id, key.clone());
                        Some(key)
                    }
                    "DeleteEntity" | "RetractDecree" => {
                        id_key.get(body["id"].as_str().unwrap_or("")).cloned()
                    }
                    "MergeEntities" => id_key.get(body["absorb"].as_str().unwrap_or("")).cloned(),
                    _ => None,
                };
                if let Some(key) = key {
                    history.entry(key).or_default().push(Event {
                        generation: entry.generation,
                        class,
                        goal: goal.clone(),
                        justification: justification.clone(),
                    });
                }
            }
        }
    }
    let mut findings = Vec::new();
    let mut parked = Vec::new();
    for (key, events) in history {
        let flips = events
            .windows(2)
            .filter(|w| w[0].class != w[1].class)
            .count();
        if flips < 2 {
            continue;
        }
        let last = events.last().unwrap();
        let gc = events
            .iter()
            .rev()
            .find(|e| e.class == goals::Class::Gc)
            .unwrap();
        let compile = events
            .iter()
            .rev()
            .find(|e| e.class == goals::Class::Compile)
            .unwrap();
        let subject = id_key
            .iter()
            .filter(|(id, k)| **k == key && store.graph.entities.contains_key(*id))
            .map(|(id, _)| id.clone())
            .next()
            .or_else(|| {
                goals::parse_goal_id(&compile.goal)
                    .map(|(_, t)| t.to_string())
                    .filter(|t| store.graph.entities.contains_key(t))
            });
        let Some(subject) = subject else { continue };
        // A ruling already given, or a filing newer than the last flip, ends it.
        let settled = store.graph.diagnostics.values().any(|d| {
            d.rule == "unstable-derivation"
                && d.subjects.iter().any(|s| *s == subject)
                && (d.answer.is_some()
                    || d.updated
                        .as_deref()
                        .and_then(|b| b.strip_prefix('g'))
                        .and_then(|n| n.parse::<u64>().ok())
                        .is_some_and(|n| n >= last.generation && d.lifecycle != "open"))
        });
        if settled {
            continue;
        }
        let name = key.split('|').next().unwrap_or(&key).to_string();
        let prompt = DiagnosticPrompt {
            question: format!(
                "`{}` flips between the classes: {} ({}) and {} ({}). Which direction holds?",
                name,
                gc.goal,
                gc.class.name(),
                compile.goal,
                compile.class.name()
            ),
            options: vec![
                PromptOption {
                    label: format!("Keep the split ({})", gc.goal),
                    edit: None,
                    answer: Some(format!(
                        "Keep the structure {} derived; the merge is wrong.",
                        gc.goal
                    )),
                },
                PromptOption {
                    label: format!("Keep the merge ({})", compile.goal),
                    edit: None,
                    answer: Some(format!(
                        "Keep the merge {} made; the split is wrong.",
                        compile.goal
                    )),
                },
            ],
            freeform: true,
        };
        findings.push((
            "unstable-derivation".to_string(),
            vec![subject.clone()],
            "warning".to_string(),
            format!(
                "`{}` was handed back and forth between {} (g{}: {}) and {} (g{}: {}); both goals are parked until a ruling",
                name,
                gc.goal,
                gc.generation,
                if gc.justification.is_empty() { "no justification recorded" } else { &gc.justification },
                compile.goal,
                compile.generation,
                if compile.justification.is_empty() { "no justification recorded" } else { &compile.justification }
            ),
            Some(prompt),
        ));
        for (goal, gen) in [
            (&gc.goal, gc.generation),
            (&compile.goal, compile.generation),
        ] {
            let Some((kind, target)) = goals::parse_goal_id(goal) else {
                continue;
            };
            let class = goals::kind(kind)
                .map(|k| k.class())
                .unwrap_or(goals::Class::Compile);
            parked.push(Goal {
                id: goal.clone(),
                kind: kind.to_string(),
                class: class.name().to_string(),
                mandatory: class == goals::Class::Compile,
                target: target.to_string(),
                unit: goals::kind(kind)
                    .map(|k| k.unit())
                    .unwrap_or("node")
                    .to_string(),
                change: json!({"unstable": name, "subject": subject}),
                cause: Some(Cause {
                    generation: gen,
                    mutation: 0,
                    via: "flip-detection".into(),
                }),
                state: GoalState::Parked,
                hints: vec![format!(
                    "parked by flip detection; answer the unstable-derivation prompt on {}",
                    subject
                )],
            });
        }
    }
    (findings, parked)
}

// Apply what a derivation learned to the store: records whose evidence lapsed, failed
// entries whose subject changed again, parked entries whose target is gone.
fn absorb_derivation(store: &mut Store, board: &Board) {
    if !board.lapsed.is_empty() {
        store.clear_changes(&board.lapsed);
    }
    if !board.reopened.is_empty() || !board.dropped_parked.is_empty() {
        store
            .status
            .failed
            .retain(|f| !board.reopened.contains(&f.goal.id));
        store
            .status
            .parked
            .retain(|p| !board.dropped_parked.contains(&p.id));
        store.save_status();
    }
}

fn kinds_line(board: &Board) -> Vec<(String, usize)> {
    let c = board.counts();
    let mut kinds: Vec<(String, usize)> = c.by_kind.into_iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    kinds
}

// The `gc burst:` count against the limit for one GC goal.
fn burst_measure(g: &Goal) -> (u64, u64, String) {
    if let Some(l) = g.change["limits"].as_array().and_then(|a| a.first()) {
        return (
            l["count"].as_u64().unwrap_or(0),
            l["soft"].as_u64().unwrap_or(0),
            l["limit"].as_str().unwrap_or("").to_string(),
        );
    }
    match g.kind.as_str() {
        "declare-edges" => (
            g.change["entities"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0) as u64,
            1,
            "entities without edges".into(),
        ),
        "dedupe-candidates" => (
            (g.change["score"].as_f64().unwrap_or(0.0) * 100.0) as u64,
            (goals::LOOKALIKE_THRESHOLD * 100.0) as u64,
            "lookalike score, percent".into(),
        ),
        _ => (
            g.change["matched"].as_array().map(|a| a.len()).unwrap_or(0) as u64,
            0,
            "matched nodes".into(),
        ),
    }
}

fn reload(
    store: &mut Store,
    out: &Path,
    parsed: &BTreeMap<String, (String, BTreeMap<String, Section>)>,
) {
    *store = Store::load(out);
    store.sync_docs(parsed);
}

pub fn compile(proj: &Project, llm: &Llm, out: &Path, trace: &Trace) -> BuildReport {
    let refused = |e: String| {
        trace.line("build", &format!("build refused: {}", e));
        BuildReport {
            verdict: "incomplete".into(),
            errors: 1,
            ..Default::default()
        }
    };
    // The control plane's build contract: one coarse lease for the run, refused while
    // an agent is mid-task, and the run itself is an approval in manual mode.
    let _build = match crate::control::begin_internal_build(proj, out, "compile") {
        Ok(g) => g,
        Err(e) => return refused(e),
    };
    // Every session runs as an ACP worker session against the configured agent; the
    // agent process lives for the run and spawns on the first session.
    let runner = match crate::acp::runner::AcpRunner::start(proj, llm, out) {
        Ok(r) => r,
        Err(e) => return refused(e),
    };
    runner.set_build_token(Some(format!("internal-{}", std::process::id())));
    trace.line("build", &format!("agent: {}", runner.agent().name));

    let mut store = Store::open_for_build(out);
    store.status.costs = Costs::default();
    store.save_status();
    let (parsed, _links) = parse_all(proj);
    store.sync_docs(&parsed);
    // Stamp the documents matching the project roots, so commits with no Project at
    // hand (edits, answers, MCP sessions) order documents by the same link levels.
    // Mirrors docs/compiler/graph.md#storage-layout.
    store.status.roots = store
        .docs
        .keys()
        .filter(|d| proj.is_root_file(d))
        .cloned()
        .collect();
    store.save_status();
    sweep(&mut store, trace);
    // A typed command is an approval (docs/compiler/control-plane.md#modes-and-releases):
    // record the compile release so manual mode gates nothing this run asked for.
    crate::control::release(proj, out, Some("compile"));
    let control = Control::load(proj, out);
    crate::derive::record_ledger_stale(&mut store, &crate::gen::GenSettings::resolve(proj));
    let mut board = Board::derive(&store, proj, &control);
    absorb_derivation(&mut store, &board);
    trace.event(TraceEvent::Board {
        label: "build".into(),
        goals: board.open_goals().len(),
        kinds: kinds_line(&board),
        blocked: board.counts().blocked,
    });
    let derived = board.open_goals().len();
    let cap = crate::limits::BUILD_SESSION_FACTOR * derived + crate::limits::BUILD_SESSION_FLOOR;

    let mut sessions = 0u32;
    let mut applied = 0usize;
    let mut costs = Costs::default();
    let mut attempts: BTreeMap<String, u32> = BTreeMap::new();
    let mut parked: Vec<Goal> = Vec::new();
    let mut parked_ids: BTreeSet<String> = BTreeSet::new();
    let mut known: BTreeSet<String> = board.open_goals().iter().map(|g| g.id.clone()).collect();

    loop {
        // The batches the serving can run: none of their goals parked this build.
        let runnable = |b: &crate::board::Batch| !b.goals.iter().any(|id| parked_ids.contains(id));
        let compile_batches: Vec<crate::board::Batch> = board
            .batches
            .iter()
            .filter(|b| b.class == goals::Class::Compile && runnable(b))
            .cloned()
            .collect();
        let gc_batches: Vec<crate::board::Batch> = board
            .batches
            .iter()
            .filter(|b| b.class == goals::Class::Gc && runnable(b))
            .cloned()
            .collect();
        let remaining = cap.saturating_sub(sessions as usize);
        // Compile outranks GC when the cap is tight.
        let tight = remaining <= compile_batches.len();
        let next = if !gc_batches.is_empty() && !tight {
            gc_batches.into_iter().next()
        } else {
            compile_batches.into_iter().next().or_else(|| {
                if tight {
                    None
                } else {
                    gc_batches.into_iter().next()
                }
            })
        };
        let Some(batch) = next else { break };
        if trace.is_cancelled() || sessions as usize >= cap {
            // Exhaustion parks every open goal the loop could still run.
            for g in board.open_goals() {
                if !parked_ids.contains(&g.id) {
                    parked_ids.insert(g.id.clone());
                    parked.push(g.clone());
                    trace.event(TraceEvent::Goal {
                        label: "build".into(),
                        goal: g.id.clone(),
                        event: "parked".into(),
                        cause: None,
                        justification: None,
                        reason: Some(if trace.is_cancelled() {
                            "the build was cancelled".into()
                        } else {
                            format!("the build cap of {} sessions ran out", cap)
                        }),
                    });
                }
            }
            break;
        }
        trace.event(TraceEvent::BatchStart {
            label: batch.id.clone(),
            class: batch.class.name().into(),
            tier: batch.tier,
            goals: batch
                .goals
                .iter()
                .filter_map(|id| board.goal(id))
                .map(|g| json!({"id": g.id, "kind": g.kind, "target": g.target}))
                .collect(),
            executor: batch.executor.clone(),
        });
        if batch.class == goals::Class::Gc {
            for g in batch.goals.iter().filter_map(|id| board.goal(id)) {
                let (count, limit, what) = burst_measure(g);
                trace.event(TraceEvent::GcBurst {
                    label: batch.id.clone(),
                    goal_kind: g.kind.clone(),
                    target: g.target.clone(),
                    count,
                    limit,
                    detail: what,
                });
            }
        }
        let gen_before = crate::store::read_generation(out);
        let batch_kind = batch
            .goals
            .first()
            .and_then(|id| board.goal(id))
            .map(|g| (g.kind.clone(), g.class.clone()))
            .unwrap_or_default();
        // One session per batch: the runner resolves the executor, ships the
        // assembled prompt, and the serving commits under its own gates.
        let run = crate::acp::runner::BatchRun {
            id: batch.id.clone(),
            goals: batch
                .goals
                .iter()
                .filter_map(|id| board.goal(id))
                .cloned()
                .collect(),
            executor: batch.executor.clone(),
        };
        sessions += 1;
        let report = runner.run_item(&run, trace);
        applied += report.applied;
        costs.charge(&batch_kind.0, &batch_kind.1, report.tokens);
        if let Some(e) = report.failed {
            trace.event(TraceEvent::SessionFailed {
                label: batch.id.clone(),
                goals: batch.goals.clone(),
                attempt: attempts.get(&batch.goals[0]).copied().unwrap_or(0) + 1,
                error: e,
            });
        } else if report.applied == 0 {
            trace.line(&batch.id, "no mutations staged");
        }
        // The session's commits live on disk; the store catches up, the sweep runs, the
        // board re-derives, and the goals the commit opened are recorded on its entry.
        reload(&mut store, out, &parsed);
        sweep(&mut store, trace);
        let before = board;
        crate::derive::record_ledger_stale(&mut store, &crate::gen::GenSettings::resolve(proj));
        board = Board::derive(&store, proj, &control);
        absorb_derivation(&mut store, &board);
        let gen_after = crate::store::read_generation(out);
        let resolved_in_journal: BTreeMap<String, String> = goals::read_journal(out)
            .iter()
            .filter(|e| e.generation > gen_before && e.generation <= gen_after)
            .flat_map(|e| {
                e.resolved_goals
                    .iter()
                    .map(|r| (r.goal.clone(), r.justification.clone()))
            })
            .collect();
        for id in &batch.goals {
            let still_open = board.open(id);
            if !still_open || resolved_in_journal.contains_key(id) {
                let justification = resolved_in_journal
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| "the goal no longer derives".into());
                trace.event(TraceEvent::Goal {
                    label: batch.id.clone(),
                    goal: id.clone(),
                    event: "resolved".into(),
                    cause: None,
                    justification: Some(justification),
                    reason: None,
                });
                let ids = before.records_of(id);
                if !ids.is_empty() {
                    store.clear_changes(&ids);
                }
                attempts.remove(id);
                continue;
            }
            if let Some(GoalState::Failed { reason }) = board.goal(id).map(|g| g.state.clone()) {
                trace.event(TraceEvent::Goal {
                    label: batch.id.clone(),
                    goal: id.clone(),
                    event: "failed".into(),
                    cause: None,
                    justification: None,
                    reason: Some(reason),
                });
                continue;
            }
            let n = attempts.entry(id.clone()).or_insert(0);
            *n += 1;
            if *n >= 2 {
                if let Some(g) = board.goal(id) {
                    parked_ids.insert(id.clone());
                    parked.push(g.clone());
                    trace.event(TraceEvent::Goal {
                        label: batch.id.clone(),
                        goal: id.clone(),
                        event: "parked".into(),
                        cause: None,
                        justification: None,
                        reason: Some(format!("left open after its retry in session {}", batch.id)),
                    });
                }
            }
        }
        if !resolved_in_journal.is_empty() || !board.lapsed.is_empty() {
            reload(&mut store, out, &parsed);
            board = Board::derive(&store, proj, &control);
        }
        let opened: Vec<OpenedGoal> = board
            .open_goals()
            .iter()
            .filter(|g| !known.contains(&g.id))
            .map(|g| OpenedGoal {
                goal: g.id.clone(),
                cause: g.cause.clone().unwrap_or_default(),
            })
            .collect();
        if !opened.is_empty() {
            let last_session = goals::read_journal(out)
                .iter()
                .filter(|e| {
                    e.kind == "session" && e.generation > gen_before && e.generation <= gen_after
                })
                .map(|e| e.generation)
                .max()
                .unwrap_or(gen_after);
            for o in &opened {
                trace.event(TraceEvent::Goal {
                    label: batch.id.clone(),
                    goal: o.goal.clone(),
                    event: "opened".into(),
                    cause: Some(o.cause.clone()),
                    justification: None,
                    reason: None,
                });
            }
            store.record_opened_goals(last_session, opened);
        }
        known = board.open_goals().iter().map(|g| g.id.clone()).collect();
        // The session already printed its own done line; this is the board after the
        // commit and the re-derivation, one note per batch.
        trace.event(TraceEvent::Note {
            label: batch.id.clone(),
            text: format!(
                "board: {} open, {} blocked",
                board.counts().open,
                board.counts().blocked
            ),
            verbose: false,
        });
    }

    let mut report = finalize(&mut store, proj, parked, &costs, trace);
    report.goals = derived;
    report.sessions = sessions;
    report.applied = applied;
    report
}

// The deterministic sweep and the stranded-diagnostic settle, traced.
fn sweep(store: &mut Store, trace: &Trace) {
    let gc_actions = store.gc();
    if !gc_actions.is_empty() {
        trace.line("build", &format!("gc: {}", gc_actions.join("; ")));
    }
    let settled = store.settle_dangling_diags();
    if !settled.is_empty() {
        trace.line("build", &format!("settle: {}", settled.join("; ")));
    }
}

// The deterministic tail of a build: the checks, flip detection, the verdict with its
// counts, costs, rendering, docsgen, status. No model anywhere, so whichever consumer
// empties the board runs it, the internal loop and the MCP serving alike.
// Mirrors docs/compiler/compilation.md#a-build.
pub fn finalize(
    s: &mut Store,
    proj: &Project,
    parked: Vec<Goal>,
    costs: &Costs,
    trace: &Trace,
) -> BuildReport {
    let settled = s.settle_dangling_diags();
    if !settled.is_empty() {
        trace.line("build", &format!("settle: {}", settled.join("; ")));
    }
    // Parked goals persist whole, deduplicated by id; flip detection adds its pairs.
    let (flips, flipped) = flip_detection(s);
    for g in parked.into_iter().chain(flipped) {
        match s.status.parked.iter_mut().find(|p| p.id == g.id) {
            Some(p) => *p = g,
            None => s.status.parked.push(g),
        }
    }
    s.status
        .parked
        .retain(|p| !s.status.failed.iter().any(|f| f.goal.id == p.id));
    let mut findings = checks(s, proj);
    findings.extend(flips);
    let placement = flow_placement(s);
    s.reconcile_check_diags(findings);
    // Flow placement feeds curate-view: one record per finding on the best-fit view.
    let generation = s.status.generation;
    let mut index = s
        .status
        .changes
        .iter()
        .filter(|c| c.generation == generation)
        .count();
    let mut wrote = false;
    for ((rule, subjects, _, _, _), best, alternatives, facet) in placement {
        let Some(view) = best else { continue };
        let Some(rid) = subjects.first().cloned() else {
            continue;
        };
        let diagnostic = s
            .graph
            .diagnostics
            .iter()
            .find(|(_, d)| d.lifecycle == "open" && d.rule == rule && d.subjects == subjects)
            .map(|(id, _)| id.clone());
        let shared: Vec<String> = s
            .graph
            .requirements
            .get(&rid)
            .map(|r| {
                r.entities
                    .iter()
                    .map(|e| s.resolve_id(e).to_string())
                    .collect()
            })
            .unwrap_or_default();
        let standing = s.status.changes.iter().any(|c| {
            c.kind == goals::CHANGE_FLOW_UNPLACED
                && c.subject == view
                && c.detail["requirement"] == rid
        });
        if standing {
            continue;
        }
        index += 1;
        s.status.changes.push(
            ChangeRecord::new(
                generation,
                index,
                0,
                goals::CHANGE_FLOW_UNPLACED,
                &view,
                "flow-placement",
            )
            .with_detail(json!({
                "requirement": rid,
                "facet": facet,
                "diagnostic": diagnostic,
                "shared": shared,
                "alternatives": alternatives,
            })),
        );
        wrote = true;
    }
    // A record whose requirement got placed or excluded since clears on its own.
    let flow_ids: BTreeSet<String> = s
        .graph
        .views
        .values()
        .filter(|v| goals::is_flow_kind(&v.kind))
        .flat_map(|v| {
            v.members.iter().cloned().chain(
                v.excluded
                    .iter()
                    .filter(|x| !x.note.trim().is_empty())
                    .map(|x| x.id.clone()),
            )
        })
        .collect();
    let before = s.status.changes.len();
    s.status.changes.retain(|c| {
        c.kind != goals::CHANGE_FLOW_UNPLACED
            || c.detail["requirement"]
                .as_str()
                .is_some_and(|r| s.graph.requirements.contains_key(r) && !flow_ids.contains(r))
    });
    wrote |= s.status.changes.len() != before;
    if wrote {
        s.save_status();
    }

    // The verdict: the board after the checks, blocked and optional riding as counts.
    crate::derive::record_ledger_stale(s, &crate::gen::GenSettings::resolve(proj));
    let control = Control::load(proj, &proj.out);
    let board = Board::derive(s, proj, &control);
    absorb_derivation(s, &board);
    let mut verdict = board.verdict();
    if verdict.converged() && !flip_pending(s) {
        verdict.state = "converged".into();
    }
    s.status.verdict = verdict;
    for (k, line) in &costs.by_kind {
        let c = s.status.costs.by_kind.entry(k.clone()).or_default();
        c.sessions += line.sessions;
        c.tokens += line.tokens;
    }
    for (k, line) in &costs.by_class {
        let c = s.status.costs.by_class.entry(k.clone()).or_default();
        c.sessions += line.sessions;
        c.tokens += line.tokens;
    }
    s.status.costs.sessions += costs.sessions;
    s.status.costs.tokens += costs.tokens;

    let n = crate::docsgen::write_all(s, &crate::gen::GenSettings::resolve(proj));
    if n > 0 {
        trace.line("build", &format!("docsgen: {} requirements document(s)", n));
    }
    let rendered = crate::render::render_all(s, &s.out);
    if !rendered.rendered.is_empty() || !rendered.failed.is_empty() {
        trace.line(
            "build",
            &format!(
                "render: {} view(s) drawn, {} unchanged, {} failed",
                rendered.rendered.len(),
                rendered.skipped.len(),
                rendered.failed.len()
            ),
        );
        for (id, e) in &rendered.failed {
            trace.line("build", &format!("render failed: {}: {}", id, e));
        }
    }
    // Add this build's process delta: spent.tokens is cumulative across builds and
    // processes, and llm::tokens_spent() is a process-lifetime counter, so an
    // assignment would clobber the spend of builds this process did not run.
    s.status.spent.tokens += crate::llm::take_tokens_delta();
    s.status.diagnostics = s.open_diag_counts();
    s.save_status();
    trace.line("build", &s.status.verdict.to_string());

    let (mut errors, mut warnings) = (0usize, 0usize);
    for d in s.graph.diagnostics.values() {
        if d.lifecycle == "open" && d.triage.as_deref() != Some("suppressed") {
            match d.severity.as_str() {
                "error" => errors += 1,
                "warning" => warnings += 1,
                _ => {}
            }
        }
    }
    let (mut total_secs, mut covered_secs) = (0usize, 0usize);
    for rec in s.docs.values() {
        for (r, sec) in &rec.sections {
            if !goals::section_has_body(sec) {
                continue;
            }
            total_secs += 1;
            if rec.coverage.contains_key(r) {
                covered_secs += 1;
            }
        }
    }
    let v = &s.status.verdict;
    BuildReport {
        verdict: v.to_string(),
        goals: 0,
        sessions: 0,
        applied: 0,
        parked: s.status.parked.len(),
        failed: v.failed as usize,
        blocked: v.blocked as usize,
        optional: v.optional as usize,
        errors,
        warnings,
        coverage_pct: if total_secs == 0 {
            100
        } else {
            (covered_secs * 100 / total_secs) as u32
        },
        tokens: s.status.costs.tokens,
    }
}

// An unstable-derivation ruling still owed keeps the pair parked.
fn flip_pending(s: &Store) -> bool {
    s.graph
        .diagnostics
        .values()
        .any(|d| d.lifecycle == "open" && d.rule == "unstable-derivation" && d.answer.is_none())
}

// ---- ripple: the causality DAG over the journal ----

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct RippleNode {
    pub generation: u64,
    pub kind: String,
    pub batch: Vec<String>,
    pub label: String,
    pub summary: String,
    pub resolved: Vec<Resolved>,
    pub opened: Vec<OpenedGoal>,
    pub recomputed: Vec<String>,
    pub children: Vec<RippleNode>,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct RippleTree {
    pub root: RippleNode,
    pub sessions: u64,
    pub tokens: u64,
    pub recomputes: usize,
    pub by_kind: BTreeMap<String, CostLine>,
    pub parked: Vec<Goal>,
    pub failed: Vec<FailedGoal>,
    pub verdict: String,
}

fn entry_label(entry: &JournalEntry) -> String {
    match entry.kind.as_str() {
        "session" => entry
            .batch
            .iter()
            .filter_map(|g| goals::parse_goal_id(g))
            .map(|(k, t)| format!("{} {}", k, t))
            .collect::<Vec<_>>()
            .join(", "),
        "edit" => format!(
            "{} (human)",
            entry.dirtied.first().cloned().unwrap_or_default()
        ),
        k => k.to_string(),
    }
}

fn entry_summary(entry: &JournalEntry) -> String {
    if !entry.resolved_goals.is_empty() {
        return entry
            .resolved_goals
            .iter()
            .map(|r| r.justification.clone())
            .filter(|j| !j.is_empty())
            .collect::<Vec<_>>()
            .join("; ");
    }
    let mut ops: BTreeMap<String, usize> = BTreeMap::new();
    for m in &entry.mutations {
        let name = m
            .as_object()
            .and_then(|o| o.keys().next().cloned())
            .or_else(|| m["op"].as_str().map(String::from))
            .unwrap_or_else(|| "mutation".into());
        *ops.entry(name).or_insert(0) += 1;
    }
    ops.iter()
        .map(|(k, n)| format!("{} {}", n, k))
        .collect::<Vec<_>>()
        .join(", ")
}

fn recomputed_in(entry: &JournalEntry) -> Vec<String> {
    entry
        .mutations
        .iter()
        .filter_map(|m| {
            let o = m.as_object()?;
            let (op, body) = o.iter().next()?;
            match op.as_str() {
                "CreateRequirement" | "UpdateRequirement" | "DeleteRequirement" => {
                    let id = body["id"].as_str()?;
                    let stem = id.strip_prefix("req:")?.rsplit_once('-')?.0.to_string();
                    Some(format!("relationships and machines of {}", stem))
                }
                _ => None,
            }
        })
        .take(1)
        .collect()
}

// The entry that resolved a goal, or the first session that took it, after `after`.
fn resolver<'a>(entries: &'a [JournalEntry], goal: &str, after: u64) -> Option<&'a JournalEntry> {
    entries
        .iter()
        .filter(|e| e.generation > after)
        .find(|e| e.resolved_goals.iter().any(|r| r.goal == goal))
        .or_else(|| {
            entries
                .iter()
                .filter(|e| e.generation > after && e.kind == "session")
                .find(|e| e.batch.iter().any(|b| b == goal))
        })
}

fn forward(entries: &[JournalEntry], entry: &JournalEntry, seen: &mut BTreeSet<u64>) -> RippleNode {
    seen.insert(entry.generation);
    let mut node = RippleNode {
        generation: entry.generation,
        kind: entry.kind.clone(),
        batch: entry.batch.clone(),
        label: entry_label(entry),
        summary: entry_summary(entry),
        resolved: entry.resolved_goals.clone(),
        opened: entry.opened_goals.clone(),
        recomputed: recomputed_in(entry),
        children: Vec::new(),
    };
    for o in &entry.opened_goals {
        if let Some(child) = resolver(entries, &o.goal, entry.generation) {
            if seen.contains(&child.generation) {
                continue;
            }
            node.children.push(forward(entries, child, seen));
        }
    }
    node
}

// The entry whose opened goals name what this entry resolved or took.
fn parent_of<'a>(entries: &'a [JournalEntry], entry: &JournalEntry) -> Option<&'a JournalEntry> {
    let mine: BTreeSet<&str> = entry
        .batch
        .iter()
        .map(String::as_str)
        .chain(entry.resolved_goals.iter().map(|r| r.goal.as_str()))
        .collect();
    entries
        .iter()
        .filter(|e| e.generation < entry.generation)
        .rev()
        .find(|e| {
            e.opened_goals
                .iter()
                .any(|o| mine.contains(o.goal.as_str()))
        })
}

fn touches(entry: &JournalEntry, target: &str) -> bool {
    let goal_target = |g: &str| {
        goals::parse_goal_id(g)
            .map(|(_, t)| {
                t == target
                    || t.contains(&format!("{}~", target))
                    || t.ends_with(&format!("~{}", target))
            })
            .unwrap_or(false)
    };
    entry.batch.iter().any(|g| goal_target(g))
        || entry.resolved_goals.iter().any(|r| goal_target(&r.goal))
        || entry.opened_goals.iter().any(|o| goal_target(&o.goal))
        || entry
            .dirtied
            .iter()
            .any(|d| d == target || d.starts_with(&format!("{}#", target)))
        || entry.mutations.iter().any(|m| {
            let s = m.to_string();
            s.contains(&format!("\"{}\"", target))
        })
}

// The ripple rooted at a generation, a document, or a node. `back` walks causes
// instead of consequences. Mirrors docs/frontends/cli.md#jazyk-ripple.
pub fn ripple(store: &Store, root: &str, back: bool) -> Option<RippleTree> {
    let entries = goals::read_journal(&store.out);
    if entries.is_empty() {
        return None;
    }
    let by_gen = |n: u64| entries.iter().find(|e| e.generation == n);
    let generation = root
        .strip_prefix('g')
        .unwrap_or(root)
        .parse::<u64>()
        .ok()
        .filter(|_| root.chars().all(|c| c.is_ascii_digit() || c == 'g'));
    let start: &JournalEntry = if let Some(n) = generation {
        by_gen(n)?
    } else if root.ends_with(".md") && !root.contains(':') {
        entries.iter().rev().find(|e| {
            e.kind == "edit"
                && e.dirtied
                    .iter()
                    .any(|d| d.starts_with(&format!("{}#", root)) || d == root)
        })?
    } else {
        let last = entries.iter().rev().find(|e| touches(e, root))?;
        // The last cascade that touched it: back to its root, then forward.
        let mut cur = last;
        let mut hops = 0;
        while let Some(p) = parent_of(&entries, cur) {
            cur = p;
            hops += 1;
            if hops > 64 {
                break;
            }
        }
        cur
    };
    let root_node = if back {
        let mut chain: Vec<&JournalEntry> = vec![start];
        let mut cur = start;
        let mut hops = 0;
        while let Some(p) = parent_of(&entries, cur) {
            chain.push(p);
            cur = p;
            hops += 1;
            if hops > 64 {
                break;
            }
        }
        // Render causes as a chain from the root cause down to the start.
        let mut node: Option<RippleNode> = None;
        for e in chain {
            let mut n = RippleNode {
                generation: e.generation,
                kind: e.kind.clone(),
                batch: e.batch.clone(),
                label: entry_label(e),
                summary: entry_summary(e),
                resolved: e.resolved_goals.clone(),
                opened: e.opened_goals.clone(),
                recomputed: recomputed_in(e),
                children: Vec::new(),
            };
            if let Some(child) = node.take() {
                n.children.push(child);
            }
            node = Some(n);
        }
        node.unwrap()
    } else {
        forward(&entries, start, &mut BTreeSet::new())
    };
    let mut sessions = 0u64;
    let mut tokens = 0u64;
    let mut recomputes = 0usize;
    let mut by_kind: BTreeMap<String, CostLine> = BTreeMap::new();
    fn walk(
        n: &RippleNode,
        sessions: &mut u64,
        tokens: &mut u64,
        recomputes: &mut usize,
        by_kind: &mut BTreeMap<String, CostLine>,
        entries: &[JournalEntry],
    ) {
        if n.kind == "session" {
            *sessions += 1;
            let t = entries
                .iter()
                .find(|e| e.generation == n.generation)
                .map(|e| e.tokens)
                .unwrap_or(0);
            *tokens += t;
            if let Some((k, _)) = n.batch.first().and_then(|g| goals::parse_goal_id(g)) {
                let line = by_kind.entry(k.to_string()).or_default();
                line.sessions += 1;
                line.tokens += t;
            }
        }
        *recomputes += n.recomputed.len();
        for c in &n.children {
            walk(c, sessions, tokens, recomputes, by_kind, entries);
        }
    }
    walk(
        &root_node,
        &mut sessions,
        &mut tokens,
        &mut recomputes,
        &mut by_kind,
        &entries,
    );
    Some(RippleTree {
        root: root_node,
        sessions,
        tokens,
        recomputes,
        by_kind,
        parked: store.status.parked.clone(),
        failed: store.status.failed.clone(),
        verdict: store.status.verdict.to_string(),
    })
}

// Plain text, one line per journal entry, indented per goal.
pub fn render_ripple(tree: &RippleTree) -> String {
    fn line(n: &RippleNode) -> String {
        let mut s = format!("{} g{}", n.label, n.generation);
        if !n.summary.is_empty() {
            s.push_str(&format!(": {}", n.summary));
        }
        s
    }
    fn walk(n: &RippleNode, prefix: &str, out: &mut String) {
        for r in &n.recomputed {
            out.push_str(&format!("{}│  recomputed at commit: {}\n", prefix, r));
        }
        let count = n.children.len();
        for (i, c) in n.children.iter().enumerate() {
            let last = i + 1 == count;
            out.push_str(&format!(
                "{}{} {}\n",
                prefix,
                if last { "└─" } else { "├─" },
                line(c)
            ));
            let next = format!("{}{}", prefix, if last { "   " } else { "│  " });
            walk(c, &next, out);
        }
    }
    let mut out = String::new();
    out.push_str(&format!(
        "{} {}\n",
        tree.root.kind,
        line(&tree.root).trim_start_matches(&format!("{} ", tree.root.kind))
    ));
    walk(&tree.root, "", &mut out);
    let gc: usize = tree
        .by_kind
        .iter()
        .filter(|(k, _)| goals::kind(k).is_some_and(|x| x.class() == goals::Class::Gc))
        .map(|(_, l)| l.sessions as usize)
        .sum();
    out.push_str(&if gc == 0 {
        "gc: no goals derived\n".to_string()
    } else {
        format!("gc: {} session(s)\n", gc)
    });
    let tokens = if tree.tokens >= 1000 {
        format!("{}k", tree.tokens / 1000)
    } else {
        tree.tokens.to_string()
    };
    out.push_str(&format!(
        "{}: {} sessions, {} recomputes, {} tokens\n",
        tree.verdict, tree.sessions, tree.recomputes, tokens
    ));
    for p in &tree.parked {
        out.push_str(&format!("parked: {}\n", p.id));
    }
    for f in &tree.failed {
        out.push_str(&format!("failed: {} ({})\n", f.goal.id, f.reason));
    }
    if tree.parked.is_empty() && tree.failed.is_empty() {
        out.push_str("nothing parked or failed\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{hash_hex, DocRecord, Requirement, SourceRef};
    use crate::project::Project;
    use crate::store::Store;
    use serde_json::Value;
    use std::collections::BTreeMap;

    fn seed_doc(store: &mut Store, doc: &str, text: &str) {
        store.docs.insert(
            doc.to_string(),
            DocRecord {
                content_hash: hash_hex(text),
                sections: crate::md::parse_sections(text),
                coverage: BTreeMap::new(),
            },
        );
    }

    fn req(statement: &str, entity: &str, doc: &str, section: &str, quote: &str) -> Requirement {
        Requirement {
            statement: statement.into(),
            entities: vec![entity.into()],
            edges: Vec::new(),
            source: Some(SourceRef {
                doc: doc.into(),
                section: section.into(),
                quote: quote.into(),
            }),
            ..Default::default()
        }
    }

    fn rules_for<'a>(f: &'a [Finding], rule: &str) -> Vec<&'a Finding> {
        f.iter().filter(|(r, _, _, _, _)| r == rule).collect()
    }

    #[test]
    fn pinned_literals_picks_values_and_skips_words() {
        let lits = pinned_literals(
            "The gateway shall log to `/var/log/gw.log` using model `us.claude-4` while the `username` field stays unique and `--verbose` widens it.",
        );
        assert!(lits.contains(&"/var/log/gw.log".to_string()), "{:?}", lits);
        assert!(lits.contains(&"us.claude-4".to_string()), "{:?}", lits);
        assert!(lits.contains(&"--verbose".to_string()), "{:?}", lits);
        assert!(!lits.iter().any(|l| l == "username"), "{:?}", lits);
    }

    #[test]
    fn drift_check_flags_missing_literals_with_a_question() {
        use crate::gen::{Ledger, ReqRow, RowHashes, TestRef};
        let dir = std::env::temp_dir().join(format!("jazyk-drift-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("product")).unwrap();
        std::fs::write(
            dir.join("jazyk.toml"),
            "[docs]\nglob = [\"docs/**/*.md\"]\n\n[gen]\ndeliverable = \"./product\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("product/gw.rs"),
            "fn log() { /* writes to /tmp/other.log */ }\n",
        )
        .unwrap();
        let proj = Project::load(&dir);
        let mut store = Store {
            out: proj.out.clone(),
            ..Default::default()
        };
        store.graph.requirements.insert(
            "req:gw-1".into(),
            req(
                "The gateway shall log to `/var/log/gw.log`.",
                "ent:gw",
                "docs/gw.md",
                "/gw",
                "logs",
            ),
        );
        store.graph.requirements.insert(
            "req:gw-2".into(),
            req(
                "The gateway shall keep `/tmp/other.log` rotating.",
                "ent:gw",
                "docs/gw.md",
                "/gw",
                "rotates",
            ),
        );
        let row = |_: ()| ReqRow {
            entity: "ent:gw".into(),
            files: vec!["gw.rs".into()],
            sites: Vec::new(),
            test: TestRef {
                kind: "programmatic".into(),
                label: "unit".into(),
                artifact: "gw.rs".into(),
                name: "t".into(),
                run: "true".into(),
                cwd: ".".into(),
            },
            hashes: RowHashes::default(),
            verdict: "none".into(),
            last_run: None,
            exit_code: None,
            evidence: None,
        };
        let mut ledger = Ledger::default();
        ledger.requirements.insert("req:gw-1".into(), row(()));
        ledger.requirements.insert("req:gw-2".into(), row(()));
        std::fs::create_dir_all(&proj.out).unwrap();
        ledger.save(&proj.out);

        let f = drift_checks(&store, &proj);
        assert_eq!(
            f.len(),
            1,
            "{:?}",
            f.iter().map(|x| &x.1).collect::<Vec<_>>()
        );
        let (rule, subject, sev, msg, prompt) = &f[0];
        assert_eq!(rule, "pinned-fact-drift");
        assert_eq!(*subject, ["req:gw-1"]);
        assert_eq!(sev, "warning");
        assert!(msg.contains("/var/log/gw.log"), "{}", msg);
        let p = prompt.as_ref().expect("the finding carries its question");
        assert_eq!(p.options.len(), 2);
        assert!(p.freeform);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_file_flagged_with_zero_llm_calls() {
        let mut s = Store::default();
        seed_doc(&mut s, "empty.md", "");
        seed_doc(&mut s, "blank.md", "\n\n  \n");
        seed_doc(&mut s, "full.md", "# Full\nThe system shall respond.\n");
        let f = checks(&s, &Project::default());
        let hits = rules_for(&f, "empty-file");
        assert_eq!(hits.len(), 2, "{:?}", hits);
        assert!(hits
            .iter()
            .all(|(_, subj, sev, _, _)| sev == "warning" && *subj != ["full.md"]));
    }

    #[test]
    fn broken_link_flagged_only_for_missing_md_targets() {
        let mut s = Store::default();
        seed_doc(
            &mut s,
            "a.md",
            "# A\nSee [b](./b.md) and [gone](./no-such-doc-xyz.md).\n",
        );
        seed_doc(&mut s, "b.md", "# B\ncontent\n");
        let f = checks(&s, &Project::default());
        let hits = rules_for(&f, "broken-link");
        assert_eq!(hits.len(), 1, "{:?}", hits);
        assert_eq!(hits[0].1, ["a.md"]);
        assert!(hits[0].3.contains("no-such-doc-xyz.md"));
    }

    #[test]
    fn duplicate_requirement_splits_warning_and_info() {
        let mut s = Store::default();
        s.graph.requirements.insert(
            "req:a-1".into(),
            req(
                "The system shall archive completed orders.",
                "ent:one",
                "a.md",
                "/a",
                "archives completed orders",
            ),
        );
        s.graph.requirements.insert(
            "req:a-2".into(),
            req(
                "The system shall archive completed orders.",
                "ent:one",
                "a.md",
                "/a",
                "archives completed orders",
            ),
        );
        s.graph.requirements.insert(
            "req:b-1".into(),
            req(
                "The store shall mint every id at creation.",
                "ent:two",
                "a.md",
                "/a",
                "mints every id at creation",
            ),
        );
        s.graph.requirements.insert(
            "req:b-2".into(),
            req(
                "The store shall mint every id at creation.",
                "ent:two",
                "b.md",
                "/b",
                "the store mints every id",
            ),
        );
        s.graph.requirements.insert(
            "req:c-1".into(),
            req(
                "The record shall have an id field.",
                "ent:three",
                "c.md",
                "/c",
                "- `id` - the identifier",
            ),
        );
        s.graph.requirements.insert(
            "req:c-2".into(),
            req(
                "The record shall have a name field.",
                "ent:three",
                "c.md",
                "/c",
                "- `name` - the display name",
            ),
        );
        let f = checks(&s, &Project::default());
        let hits = rules_for(&f, "duplicate-requirement");
        assert_eq!(hits.len(), 2, "{:?}", hits);
        assert!(hits
            .iter()
            .any(|(_, subj, sev, msg, _)| subj.iter().any(|s| s == "req:a-1")
                && sev == "warning"
                && msg.contains("keep one")));
        assert!(hits
            .iter()
            .any(|(_, subj, sev, msg, _)| subj.iter().any(|s| s == "req:b-1")
                && sev == "info"
                && msg.contains("both kept")));
    }

    #[test]
    fn normative_signals_catch_prose_without_shall() {
        assert!(looks_normative(
            "The user management system handles user accounts and authentication.\n"
        ));
        assert!(looks_normative(
            "Login operation can be performed by unauthenticated.\n"
        ));
        assert!(looks_normative(
            "# Operations\n- `addProduct` - adds a new product to the inventory\n"
        ));
        assert!(looks_normative("Sections shall be covered.\n"));
    }

    #[test]
    fn navigation_and_changelog_prose_stays_quiet() {
        assert!(!looks_normative(
            "See the [frontend documentation](./frontend.md) for more information.\n"
        ));
        assert!(!looks_normative(
            "# Changelog\n- 1.2: fixed typos in the intro\n"
        ));
        assert!(!looks_normative(
            "# Operations\n\nThe user management system supports the following operations:\n"
        ));
    }

    #[test]
    fn justification_closure_finds_a_fact_without_provenance() {
        let mut s = crate::derive::tests::showcase_store();
        s.graph.requirements.insert(
            "req:x-1".into(),
            Requirement {
                statement: "Invented.".into(),
                entities: vec!["ent:order".into()],
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:x-2".into(),
            Requirement {
                statement: "Derived without a proposal.".into(),
                entities: vec!["ent:order".into()],
                provenance: Some(Provenance::Derived {
                    from: vec!["ent:order".into()],
                    reasoning: "split".into(),
                }),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:x-3".into(),
            req(
                "Quoted in a dead section.",
                "ent:order",
                "shop.md",
                "/shop/gone",
                "gone",
            ),
        );
        let f = checks(&s, &Project::default());
        let hits = rules_for(&f, "unjustified-fact");
        let subjects: Vec<&str> = hits
            .iter()
            .flat_map(|h| h.1.iter().map(String::as_str))
            .collect();
        assert!(subjects.contains(&"req:x-1"), "{:?}", subjects);
        assert!(subjects.contains(&"req:x-2"), "{:?}", subjects);
        assert!(subjects.contains(&"req:x-3"), "{:?}", subjects);
        assert!(
            !subjects.contains(&"req:shop-1"),
            "a quoted fact in a live section is justified"
        );
        assert!(hits.iter().all(|h| h.2 == "error"));
    }

    #[test]
    fn flow_placement_flags_an_unplaced_behavior_and_feeds_curate_view() {
        let mut s = crate::derive::tests::showcase_store();
        // A lone behavior requirement in another document: its cluster has one member.
        let text = "# Returns\n\nreturns body\n";
        seed_doc(&mut s, "returns.md", text);
        s.graph.requirements.insert(
            "req:returns-1".into(),
            Requirement {
                statement: "The customer returns an item.".into(),
                entities: vec!["ent:customer".into()],
                facets: vec![Facet {
                    facet: "behavior".into(),
                    reasoning: "a step".into(),
                    measure: None,
                }],
                source: Some(SourceRef {
                    doc: "returns.md".into(),
                    section: "/returns".into(),
                    quote: "returns body".into(),
                }),
                ..Default::default()
            },
        );
        let mut batch = crate::store::RecordBatch::new(1);
        crate::derive::recompute(&mut s, "g1", &mut batch);
        assert!(
            s.graph.views.values().any(|v| v.kind == "use-case"),
            "the showcase derives a flow view"
        );
        let f = checks(&s, &Project::default());
        let hits = rules_for(&f, "unplaced-behavior");
        assert_eq!(hits.len(), 1, "{:?}", hits);
        assert_eq!(hits[0].1, ["req:returns-1"]);
        let placement = flow_placement(&s);
        let (_, best, _, facet) = &placement[0];
        assert_eq!(facet, "behavior");
        assert!(
            best.as_deref()
                .is_some_and(|v| v.starts_with("view:usecase/customer")),
            "{:?}",
            best
        );
    }

    #[test]
    fn provider_check_names_missing_and_ambiguous_realizers() {
        let mut s = crate::derive::tests::showcase_store();
        let mut batch = crate::store::RecordBatch::new(1);
        crate::derive::recompute(&mut s, "g1", &mut batch);
        let f = checks(&s, &Project::default());
        assert!(
            rules_for(&f, "provider-missing").is_empty(),
            "both interfaces are realized"
        );
        // Drop the realizer of the stock API: the dependency is left without a provider.
        s.graph.requirements.remove("req:shop-4");
        crate::derive::recompute(&mut s, "g2", &mut batch);
        let f = checks(&s, &Project::default());
        let missing = rules_for(&f, "provider-missing");
        assert_eq!(missing.len(), 1, "{:?}", missing);
        assert_eq!(missing[0].1, ["ent:stock-api"]);
        // Two realizers of the checkout API.
        s.graph.requirements.insert(
            "req:shop-12".into(),
            Requirement {
                statement: "The inventory service also provides the checkout API.".into(),
                entities: vec!["ent:inventory-service".into(), "ent:checkout-api".into()],
                edges: vec![ReqEdge {
                    a: "ent:inventory-service".into(),
                    b: "ent:checkout-api".into(),
                    rel_type: Some("realization".into()),
                    cardinality: None,
                }],
                source: Some(SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop".into(),
                    quote: "The shop.".into(),
                }),
                ..Default::default()
            },
        );
        crate::derive::recompute(&mut s, "g3", &mut batch);
        let f = checks(&s, &Project::default());
        let ambiguous = rules_for(&f, "provider-ambiguous");
        assert_eq!(ambiguous.len(), 1, "{:?}", ambiguous);
        assert_eq!(ambiguous[0].1, ["ent:checkout-api"]);
        // Interface-like keys on structure, not the label: with the stereotype gone,
        // the realizations alone keep the provider check on the entity.
        s.graph
            .entities
            .get_mut("ent:checkout-api")
            .unwrap()
            .stereotype = None;
        let f = checks(&s, &Project::default());
        let ambiguous = rules_for(&f, "provider-ambiguous");
        assert_eq!(ambiguous.len(), 1, "{:?}", ambiguous);
        assert_eq!(ambiguous[0].1, ["ent:checkout-api"]);
    }

    #[test]
    fn nondeterministic_transition_names_the_pair_as_subjects() {
        let m = StateMachine {
            subject: "ent:order".into(),
            states: vec!["placed".into(), "paid".into(), "held".into()],
            initial: Some("placed".into()),
            transitions: vec![
                StateTransition {
                    from: "placed".into(),
                    to: "paid".into(),
                    trigger: Some("payment succeeds".into()),
                    guard: None,
                    requirement: "req:shop-7".into(),
                },
                StateTransition {
                    from: "placed".into(),
                    to: "held".into(),
                    trigger: Some("payment succeeds".into()),
                    guard: None,
                    requirement: "req:shop-8".into(),
                },
            ],
        };
        let f = machine_checks(&m);
        let hits = rules_for(&f, "nondeterministic-transition");
        assert_eq!(hits.len(), 1, "{:?}", hits);
        assert_eq!(hits[0].1, ["req:shop-7", "req:shop-8"]);
    }

    #[test]
    fn conformance_check_flags_an_attribute_the_type_does_not_declare() {
        let mut s = crate::derive::tests::showcase_store();
        let mut batch = crate::store::RecordBatch::new(1);
        crate::derive::recompute(&mut s, "g1", &mut batch);
        let f = checks(&s, &Project::default());
        assert!(
            rules_for(&f, "nonconformant-instance").is_empty(),
            "{:?}",
            rules_for(&f, "nonconformant-instance")
        );
        s.graph
            .entities
            .get_mut("ent:ana")
            .unwrap()
            .attributes
            .push(Attribute {
                name: "shoe size".into(),
                r#type: None,
                value: Some("38".into()),
                provenance: Provenance::Quote(SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop/examples".into(),
                    quote: "examples".into(),
                }),
            });
        let f = checks(&s, &Project::default());
        let hits = rules_for(&f, "nonconformant-instance");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1, ["ent:ana"]);
        assert!(
            hits[0].3.contains("shoe size") && hits[0].3.contains("ent:customer"),
            "{}",
            hits[0].3
        );
        let sm = rules_for(&f, "dead-end-state");
        assert_eq!(sm.len(), 1, "the order machine has final states: {:?}", sm);
        assert!(rules_for(&f, "quality-unmeasured").is_empty());
    }

    fn journal_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jazyk-flip-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("journal")).unwrap();
        dir
    }

    fn write_entry(
        dir: &Path,
        generation: u64,
        batch: &[&str],
        mutations: Vec<Value>,
        justification: &str,
    ) {
        let entry = JournalEntry {
            build: format!("g{}", generation),
            generation,
            kind: "session".into(),
            batch: batch.iter().map(|s| s.to_string()).collect(),
            mutations,
            resolved_goals: batch
                .iter()
                .map(|g| Resolved {
                    goal: g.to_string(),
                    justification: justification.into(),
                    evidence: Value::Null,
                })
                .collect(),
            ..Default::default()
        };
        std::fs::write(
            dir.join("journal").join(format!("g{}.yaml", generation)),
            serde_norway::to_string(&entry).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn cross_class_flip_parks_the_pair_after_two_flips() {
        let dir = journal_dir();
        let mut s = crate::derive::tests::showcase_store();
        s.out = dir.clone();
        let create = |id: &str| json!({"CreateEntity": {"id": id, "entity": {"name": "Order pricing", "scope": "public"}}});
        write_entry(
            &dir,
            5,
            &["g:abstract-entity:ent:order"],
            vec![create("ent:order-pricing")],
            "pricing statements cohere",
        );
        write_entry(
            &dir,
            6,
            &["g:review-entity:ent:order"],
            vec![
                json!({"MergeEntities": {"keep": "ent:order", "absorb": "ent:order-pricing", "reason": "one concept"}}),
            ],
            "pricing is the order itself",
        );
        let (f, parked) = flip_detection(&s);
        assert!(f.is_empty(), "one flip is not yet oscillation: {:?}", f);
        assert!(parked.is_empty());
        write_entry(
            &dir,
            7,
            &["g:abstract-entity:ent:order"],
            vec![create("ent:order-pricing-2")],
            "pricing statements cohere again",
        );
        let (f, parked) = flip_detection(&s);
        assert_eq!(f.len(), 1, "{:?}", f);
        let (rule, subject, _, msg, prompt) = &f[0];
        assert_eq!(rule, "unstable-derivation");
        assert_eq!(*subject, ["ent:order"]);
        assert!(
            msg.contains("pricing statements cohere again")
                && msg.contains("pricing is the order itself"),
            "{}",
            msg
        );
        assert_eq!(prompt.as_ref().unwrap().options.len(), 2);
        let ids: BTreeSet<&str> = parked.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(
            ids,
            ["g:abstract-entity:ent:order", "g:review-entity:ent:order"]
                .into_iter()
                .collect()
        );
        assert!(parked.iter().all(|g| g.state == GoalState::Parked));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ripple_walks_opened_goals_to_their_resolvers() {
        let dir = journal_dir();
        let mut s = Store::default();
        s.out = dir.clone();
        let edit = JournalEntry {
            build: "g87".into(),
            generation: 87,
            kind: "edit".into(),
            dirtied: vec!["docs/orders.md#/orders/holds".into()],
            opened_goals: vec![OpenedGoal {
                goal: "g:reconcile-section:docs/orders.md#/orders/holds".into(),
                cause: Cause {
                    generation: 87,
                    mutation: 1,
                    via: "section".into(),
                },
            }],
            ..Default::default()
        };
        std::fs::write(
            dir.join("journal/g87.yaml"),
            serde_norway::to_string(&edit).unwrap(),
        )
        .unwrap();
        let mut session = JournalEntry {
            build: "g88".into(),
            generation: 88,
            kind: "session".into(),
            batch: vec!["g:reconcile-section:docs/orders.md#/orders/holds".into()],
            mutations: vec![
                json!({"UpdateRequirement": {"id": "req:orders-6", "statement": "30 days"}}),
            ],
            resolved_goals: vec![Resolved {
                goal: "g:reconcile-section:docs/orders.md#/orders/holds".into(),
                justification: "req:orders-6 revised".into(),
                evidence: Value::Null,
            }],
            opened_goals: vec![OpenedGoal {
                goal: "g:rejudge-pair:req:orders-6~req:payment-9".into(),
                cause: Cause {
                    generation: 88,
                    mutation: 1,
                    via: "entities".into(),
                },
            }],
            tokens: 9_800,
            ..Default::default()
        };
        std::fs::write(
            dir.join("journal/g88.yaml"),
            serde_norway::to_string(&session).unwrap(),
        )
        .unwrap();
        session.generation = 89;
        session.build = "g89".into();
        session.batch = vec!["g:rejudge-pair:req:orders-6~req:payment-9".into()];
        session.mutations = Vec::new();
        session.resolved_goals = vec![Resolved {
            goal: "g:rejudge-pair:req:orders-6~req:payment-9".into(),
            justification: "consistent".into(),
            evidence: Value::Null,
        }];
        session.opened_goals = Vec::new();
        std::fs::write(
            dir.join("journal/g89.yaml"),
            serde_norway::to_string(&session).unwrap(),
        )
        .unwrap();
        let tree = ripple(&s, "g87", false).expect("root generation exists");
        assert_eq!(tree.root.generation, 87);
        assert_eq!(tree.root.children.len(), 1);
        assert_eq!(tree.root.children[0].generation, 88);
        assert_eq!(tree.root.children[0].children[0].generation, 89);
        assert_eq!(tree.sessions, 2);
        assert_eq!(tree.tokens, 19_600);
        let text = render_ripple(&tree);
        assert!(
            text.contains("edit docs/orders.md#/orders/holds (human) g87"),
            "{}",
            text
        );
        assert!(
            text.contains(
                "└─ reconcile-section docs/orders.md#/orders/holds g88: req:orders-6 revised"
            ),
            "{}",
            text
        );
        assert!(
            text.contains("└─ rejudge-pair req:orders-6~req:payment-9 g89: consistent"),
            "{}",
            text
        );
        assert!(text.contains("19k tokens"), "{}", text);
        // A node root finds the last cascade that touched it and starts at its edit.
        let by_node = ripple(&s, "req:orders-6", false).unwrap();
        assert_eq!(by_node.root.generation, 87);
        let back = ripple(&s, "89", true).unwrap();
        assert_eq!(back.root.generation, 87);
        assert_eq!(back.root.children[0].generation, 88);
        assert!(ripple(&s, "docs/orders.md", false).is_some());
        assert!(ripple(&s, "g4000", false).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_verdict_displays_its_counts() {
        let mut s = crate::derive::tests::showcase_store();
        s.status.generation = 3;
        let proj = crate::board::tests::project_for(&s);
        let board = Board::derive(&s, &proj, &crate::board::tests::control_auto());
        let v = board.verdict();
        assert_eq!(v.state, "incomplete");
        assert!(
            v.open >= 4,
            "every uncovered section is an open goal: {}",
            v.open
        );
        assert_eq!(
            v.to_string(),
            format!(
                "incomplete: {} open, 0 failed, {} blocked, 0 optional advised",
                v.open, v.blocked
            )
        );
        assert!(
            v.blocked > 0,
            "the ledger goals wait on the generate release"
        );
        assert!(
            board
                .summary_line()
                .starts_with(&format!("compile: {} goals (", v.open)),
            "{}",
            board.summary_line()
        );
    }
}
