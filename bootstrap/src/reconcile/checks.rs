// The deterministic checks over the whole graph, the state machine checks, and the
// levels: the level-shape check and the shape of the containment tree.
use super::*;

// A matched file's document path: relative to the project root, `/`-separated.
fn doc_path(proj: &Project, f: &std::path::Path) -> String {
    match f.strip_prefix(&proj.root) {
        Ok(r) => r.to_string_lossy().replace('\\', "/"),
        Err(_) => f.to_string_lossy().replace('\\', "/"),
    }
}

// Read and projection failures remain actionable; unknown formats compile as blobs.
fn format_checks(proj: &Project) -> Vec<Finding> {
    let mut f = Vec::new();
    for (rel, input) in crate::input::scan(proj) {
        if let Err(e) = input {
            f.push(finding(
                "parse-error",
                &rel,
                "error",
                format!("{} cannot be parsed: {}", rel, e),
            ));
        }
    }
    f
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
            if let Ok(t) = crate::vfs::read_to_string(gs.deliverable.join(f)) {
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
fn flow_placement_ranking(store: &Store, ix: &Indices, rid: &str) -> Vec<String> {
    let Some(r) = store.graph.requirements.get(rid) else {
        return Vec::new();
    };
    let ents: BTreeSet<String> = r
        .entities
        .iter()
        .map(|e| store.resolve_id(e).to_string())
        .collect();
    let doc = r.source.as_ref().map(|s| s.doc.clone()).unwrap_or_default();
    let position = |id: &str| {
        ix.requirement_position
            .get(id)
            .copied()
            .unwrap_or(usize::MAX)
    };
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
    // The requirement's own cluster (its document's flow at the root level) outranks
    // every other. Mirrors docs/compiler/goals/curate-view.md.
    let stem = doc
        .rsplit('/')
        .next()
        .unwrap_or(&doc)
        .trim_end_matches(".md");
    let own = format!("view:usecase/{}", md::slug(stem));
    if let Some(pos) = ids.iter().position(|v| *v == own) {
        let v = ids.remove(pos);
        ids.insert(0, v);
    }
    ids
}

// The flow placement findings, each with the view the record lands on and the
// alternatives. Runs only where flow views exist.
pub(super) fn flow_placement(
    store: &Store,
    ix: &Indices,
) -> Vec<(Finding, Option<String>, Vec<String>, String)> {
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
        let ranking = flow_placement_ranking(store, ix, rid);
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
    // The indices every check below reads, built once.
    // Mirrors docs/compiler/graph.md#derived-data.
    let ix = Indices::build(store);
    let mut f: Vec<Finding> = format_checks(proj);
    // Link diagnostics describe the current source, including uncompiled docs.
    // One stable source-target pair keeps its identity when the link moves.
    for path in proj.doc_files() {
        if !md::supported_format(&path) {
            continue;
        }
        let Ok(text) = md::read_doc(&path) else {
            continue;
        };
        let doc = doc_path(proj, &path);
        let mut reported = BTreeSet::new();
        for link in md::missing_document_links(&proj.root, &doc, &text) {
            if reported.insert(link.target.clone()) {
                f.push((
                    link.rule.into(),
                    vec![doc.clone(), link.target],
                    link.severity.into(),
                    link.message,
                    None,
                ));
            }
        }
    }
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
                Some(c)
                    if c.state == "non-normative"
                        && sec.kind != "blob"
                        && looks_normative(&sec.raw) =>
                {
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
    // Entities no requirement references and no child hangs under: a grouping is
    // used as its children's container. Mirrors docs/compiler/compilation.md#checks.
    for id in store.graph.entities.keys() {
        if !ix.has_children(id) && ix.requirements_referencing(id).is_empty() {
            f.push(finding(
                "unused-entity",
                id,
                "warning",
                format!("{} has no requirement referencing it", id),
            ));
        }
    }
    // A binding whose implementing files exist and whose tests fail: the deliverable
    // contradicts the statement, and the author hears it here, since the binding
    // loop has no batch to file into (docs/consumers/bind.md#what-the-verdict-means).
    {
        let gs = crate::gen::GenSettings::resolve(proj);
        let ledger = crate::gen::Ledger::load(&store.out);
        for (rid, row) in &ledger.requirements {
            if !store.graph.requirements.contains_key(rid) {
                continue;
            }
            let (status, reason) = crate::verify::status_with(store, rid, row, &gs, &ledger);
            if status == "failing" {
                let red: Vec<&str> = row
                    .tests
                    .iter()
                    .filter(|(_, t)| t.verdict == "fail")
                    .map(|(c, _)| c.as_str())
                    .collect();
                let message = if reason == "criteria-failing-after-generation" {
                    format!(
                        "{} keeps red criterion tests ({}) after {} was generated over the current facts: generation had its turn, so the criterion contradicts the deliverable or its neighbors (the docs are wrong, or the code is); {}",
                        rid,
                        red.join(", "),
                        row.entity,
                        row.evidence.as_deref().unwrap_or("no evidence recorded")
                    )
                } else {
                    format!(
                        "{} is implemented in {} but its test `{}` fails: the deliverable contradicts the statement (the docs are wrong, or the code is); {}",
                        rid,
                        row.files.join(", "),
                        row.test.name,
                        row.evidence.as_deref().unwrap_or("no evidence recorded")
                    )
                };
                f.push(finding("failing-binding", rid, "warning", message));
            }
        }
    }
    // Criteria: a requirement with none, and a criterion that restates its
    // requirement. Neither holds the verdict (docs/compiler/compilation.md#checks).
    let gen_configured = proj.gen_deliverable.is_some();
    for (rid, r) in &store.graph.requirements {
        if store.criteria_of(rid).is_empty() {
            f.push(finding(
                "criterion-missing",
                rid,
                if gen_configured { "warning" } else { "info" },
                format!(
                    "{} has no acceptance criterion; {}",
                    rid,
                    if gen_configured {
                        "bind waits for one (derive-criteria writes them)"
                    } else {
                        "nothing says how one would know it holds"
                    }
                ),
            ));
        }
        for cid in store.criteria_of(rid) {
            if let Some(c) = store.graph.criteria.get(&cid) {
                if crate::store::normalize_statement(&c.statement)
                    == crate::store::normalize_statement(&r.statement)
                {
                    f.push(finding(
                        "criterion-restates",
                        &cid,
                        "warning",
                        format!("{} restates {} instead of giving evidence for it", cid, rid),
                    ));
                }
            }
        }
    }
    // Two criteria about one entity that describe the same situation (given and when)
    // with different outcomes (then): the contradiction written into the evidence.
    // Mirrors docs/compiler/compilation.md#checks.
    {
        let norm = |s: &Option<String>| {
            s.as_deref()
                .map(crate::store::normalize_statement)
                .filter(|x| !x.is_empty())
        };
        let mut by_entity: BTreeMap<String, Vec<&String>> = BTreeMap::new();
        for (cid, c) in &store.graph.criteria {
            let ents = if c.entities.is_empty() {
                store
                    .graph
                    .requirements
                    .get(&c.requirement)
                    .map(|r| r.entities.clone())
                    .unwrap_or_default()
            } else {
                c.entities.clone()
            };
            for e in ents {
                by_entity.entry(e).or_default().push(cid);
            }
        }
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
        for cids in by_entity.values() {
            for (i, a) in cids.iter().enumerate() {
                for b in &cids[i + 1..] {
                    let (ca, cb) = (&store.graph.criteria[*a], &store.graph.criteria[*b]);
                    let (Some(ga), Some(wa), Some(ta)) =
                        (norm(&ca.given), norm(&ca.when), norm(&ca.then))
                    else {
                        continue;
                    };
                    let (Some(gb), Some(wb), Some(tb)) =
                        (norm(&cb.given), norm(&cb.when), norm(&cb.then))
                    else {
                        continue;
                    };
                    if ga != gb || wa != wb || ta == tb {
                        continue;
                    }
                    let key = if a < b {
                        ((*a).clone(), (*b).clone())
                    } else {
                        ((*b).clone(), (*a).clone())
                    };
                    if !seen.insert(key.clone()) {
                        continue;
                    }
                    f.push((
                        "criteria-conflict".to_string(),
                        vec![key.0.clone(), key.1.clone()],
                        "warning".to_string(),
                        format!(
                            "{} and {} describe the same situation (given {}, when {}) with different outcomes ({} versus {}); one of them, or the rule behind it, is wrong",
                            key.0,
                            key.1,
                            ca.given.as_deref().unwrap_or_default(),
                            ca.when.as_deref().unwrap_or_default(),
                            store.graph.criteria[&key.0].then.as_deref().unwrap_or_default(),
                            store.graph.criteria[&key.1].then.as_deref().unwrap_or_default()
                        ),
                        None,
                    ));
                }
            }
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
        // The neighbors of every id, once: the members of each relationship it is in
        // and the entities of each requirement listing it (as written, redirects not
        // followed, as the walk has always read them).
        let mut neighbors: BTreeMap<&str, Vec<&String>> = BTreeMap::new();
        for rel in store.graph.relationships.values() {
            for m in &rel.members {
                neighbors
                    .entry(m.as_str())
                    .or_default()
                    .extend(rel.members.iter());
            }
        }
        for r in store.graph.requirements.values() {
            for m in &r.entities {
                neighbors
                    .entry(m.as_str())
                    .or_default()
                    .extend(r.entities.iter());
            }
        }
        let mut reach = root_entities.clone();
        let mut frontier: Vec<String> = root_entities.into_iter().collect();
        while let Some(id) = frontier.pop() {
            for m in neighbors.get(id.as_str()).into_iter().flatten() {
                if reach.insert((*m).clone()) {
                    frontier.push((*m).clone());
                }
            }
            // Containment counts both ways: a level under a grouping is as
            // reachable as the grouping. Mirrors docs/compiler/compilation.md#checks.
            if let Some(p) = store.graph.entities.get(&id).and_then(|e| e.parent.clone()) {
                if reach.insert(p.clone()) {
                    frontier.push(p);
                }
            }
            for cid in ix.children_of(&id) {
                if reach.insert(cid.clone()) {
                    frontier.push(cid.clone());
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
            if sec.kind != "blob" && sec.raw.len() > crate::limits::MAX_SECTION_CHARS {
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
        // The natural key's normalization (docs/compiler/compilation.md#checks): a
        // decimal or a signed number stays one token, so `1.5` and `2.5` differ.
        let toks = |s: &str| -> BTreeSet<String> {
            crate::store::normalize_statement(s)
                .split(' ')
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
                    // A pair a session judged is its finding, not the check's: a
                    // one-subject twin beside the two-subject diagnostic is the same
                    // finding twice (docs/compiler/compilation.md#checks).
                    let judged = store.graph.diagnostics.values().any(|d| {
                        d.lifecycle == "open"
                            && (d.rule == "contradiction" || d.rule == "duplicate-requirement")
                            && d.subjects.iter().any(|s| s == &key.0)
                            && d.subjects.iter().any(|s| s == &key.1)
                    });
                    if judged {
                        continue;
                    }
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
            let digits = &suffix[1..];
            if !digits.is_empty()
                && digits.chars().all(|c| c.is_ascii_digit())
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
    for (finding, _, _, _) in flow_placement(store, &ix) {
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
    // Level shape: every level has its view, no level is over the hard fan-out, no
    // derived grouping is under-membered.
    f.extend(level_shape_checks(store, &ix));
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
    let mut contributions_to: BTreeMap<&str, Vec<&Contribution>> = BTreeMap::new();
    for c in store
        .graph
        .relationships
        .values()
        .flat_map(|r| r.contributions.iter())
    {
        contributions_to.entry(c.b.as_str()).or_default().push(c);
    }
    for (iface, e) in &store.graph.entities {
        let contributions: &[&Contribution] = contributions_to
            .get(iface.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]);
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
                // One diagnostic per pair of arrows, the contributing requirements of
                // both arrows as subjects, so deleting any one re-triggers rejudge
                // through deletion propagation.
                f.push((
                    "nondeterministic-transition".to_string(),
                    a.requirements
                        .iter()
                        .chain(&b.requirements)
                        .cloned()
                        .collect(),
                    "warning".to_string(),
                    format!(
                        "{} and {} both leave {} on {} with overlapping guards",
                        a.requirements.join(", "),
                        b.requirements.join(", "),
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

// ---- levels: the level-shape check and the shape of the tree ----

// The limit row that bounds a level. Mirrors docs/compiler/graph.md#limits.
const CHILDREN_LIMIT: &str = crate::limits::CHILDREN_PER_ENTITY;

// Every level of the store: the target (a node id, or `scope:<scope>` for the root
// form) with its direct children, id-ordered. A node with no child is not a level and
// is not listed; a scope root always is. Mirrors docs/compiler/concepts/levels.md#levels.
pub fn levels(store: &Store) -> Vec<(String, Vec<String>)> {
    let mut by_parent: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut scopes: BTreeSet<&str> = BTreeSet::new();
    for (id, e) in &store.graph.entities {
        scopes.insert(&e.scope);
        let key = match &e.parent {
            Some(p) => p.clone(),
            None => crate::store::scope_root_target(&e.scope),
        };
        by_parent.entry(key).or_default().push(id.clone());
    }
    let mut out: Vec<(String, Vec<String>)> = scopes
        .into_iter()
        .map(|s| {
            let t = crate::store::scope_root_target(s);
            let children = by_parent.remove(&t).unwrap_or_default();
            (t, children)
        })
        .collect();
    out.extend(
        by_parent
            .into_iter()
            .filter(|(p, _)| store.graph.entities.contains_key(p)),
    );
    out
}

// The slug segment of a level view id: the node's slug, the scope's slug for the root
// form (the per-scope id the root view has always had, `view:component/public`).
// Mirrors docs/compiler/model/view.md#level-views.
fn level_slug(target: &str) -> String {
    match crate::board::scope_target(target) {
        Some(scope) => md::slug(scope),
        None => crate::ids::entity_slug(target).to_string(),
    }
}

// The structural level view of a target, under either kind the kind rule can pick
// (`component` when the level carries a structural stereotype, `class` otherwise).
// Mirrors docs/compiler/model/view.md#level-views.
pub fn level_view_of(store: &Store, target: &str) -> Option<String> {
    let slug = level_slug(target);
    ["component", "class"]
        .iter()
        .map(|k| format!("view:{}/{}", k, slug))
        .find(|id| store.graph.views.contains_key(id))
}

// The (soft, hard) fan-out thresholds in force on a target: the node's own bump is its
// soft value; the scope root has no bump. Mirrors docs/compiler/graph.md#per-node-bumps.
pub fn fan_out_thresholds(store: &Store, target: &str) -> (u64, u64) {
    let bump = store
        .graph
        .entities
        .get(target)
        .and_then(|e| e.limits.get(CHILDREN_LIMIT))
        .map(|b| b.value);
    crate::limits::threshold(CHILDREN_LIMIT, bump).unwrap_or((u64::MAX, u64::MAX))
}

// A derived grouping: derived provenance, no mentions, and no requirement of its own
// (a sub-entity the caps variant minted holds statements; a grouping holds a level).
// Mirrors docs/compiler/concepts/levels.md#groupings.
pub fn is_derived_grouping(store: &Store, id: &str) -> bool {
    is_derived_grouping_with(store, &Indices::build(store), id)
}

fn is_derived_grouping_with(store: &Store, ix: &Indices, id: &str) -> bool {
    store.graph.entities.get(id).is_some_and(|e| {
        matches!(e.provenance, Some(Provenance::Derived { .. }))
            && e.mentions.is_empty()
            && ix.requirements_referencing(id).is_empty()
    })
}

// The level-shape check: a level of two or more without its structural view, a level
// over the hard fan-out threshold, a derived grouping with fewer than two children.
// Mirrors docs/compiler/compilation.md#checks.
fn level_shape_checks(store: &Store, ix: &Indices) -> Vec<Finding> {
    let mut f = Vec::new();
    for (target, children) in levels(store) {
        if children.len() >= 2 && level_view_of(store, &target).is_none() {
            f.push(finding(
                "level-shape",
                &target,
                "warning",
                format!(
                    "{} holds {} direct children but has no structural level view (view:component/{} or view:class/{})",
                    target,
                    children.len(),
                    level_slug(&target),
                    level_slug(&target)
                ),
            ));
        }
        let (_, hard) = fan_out_thresholds(store, &target);
        if children.len() as u64 > hard {
            f.push(finding(
                "level-shape",
                &target,
                "warning",
                format!(
                    "{} holds {} direct children, over the hard {} threshold of {}; the mandatory abstract-entity goal regroups the level",
                    target,
                    children.len(),
                    CHILDREN_LIMIT,
                    hard
                ),
            ));
        }
        if children.len() < 2 && is_derived_grouping_with(store, ix, &target) {
            f.push(finding(
                "level-shape",
                &target,
                "warning",
                format!(
                    "{} is a derived grouping with {} child(ren); the sweep dissolves a grouping under two",
                    target,
                    children.len()
                ),
            ));
        }
    }
    // A grouping with no child at all is not a level and never reaches the loop above.
    for id in store.graph.entities.keys() {
        if !ix.has_children(id) && is_derived_grouping_with(store, ix, id) {
            f.push(finding(
                "level-shape",
                id,
                "warning",
                format!(
                    "{} is a derived grouping with 0 child(ren); the sweep dissolves a grouping under two",
                    id
                ),
            ));
        }
    }
    f
}

// The shape of the containment tree: entities per depth (the parentless entities at
// depth 1, their children at depth 2, and so on) and the fan-out histogram, how many
// levels (the scope roots included) hold how many direct children, banded against the
// `children-per-entity` registry values: at or under soft, over soft and at or under
// hard, over hard. Mirrors docs/compiler/compilation.md#convergence.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    pub per_depth: Vec<usize>,
    pub bands: [usize; 3],
}

pub fn shape(store: &Store) -> Shape {
    let mut per_depth: Vec<usize> = Vec::new();
    for id in store.graph.entities.keys() {
        let mut depth = 1usize;
        let mut cur = id.as_str();
        while let Some(p) = store
            .graph
            .entities
            .get(cur)
            .and_then(|e| e.parent.as_deref())
        {
            depth += 1;
            cur = p;
            if depth > 64 {
                break;
            }
        }
        if per_depth.len() < depth {
            per_depth.resize(depth, 0);
        }
        per_depth[depth - 1] += 1;
    }
    let (soft, hard) = crate::limits::limit(CHILDREN_LIMIT)
        .map(|l| (l.soft, l.hard))
        .unwrap_or((u64::MAX, u64::MAX));
    let mut bands = [0usize; 3];
    for (_, children) in levels(store) {
        let n = children.len() as u64;
        if n < 2 {
            continue;
        }
        let band = if n <= soft {
            0
        } else if n <= hard {
            1
        } else {
            2
        };
        bands[band] += 1;
    }
    Shape { per_depth, bands }
}

#[cfg(test)]
mod tests;
