// The reconciler: compares the documents (desired state) against the graph (observed
// state) and schedules turns until they agree. Deterministic; the model never decides
// what is stale or what runs next. Mirrors docs2/compiler/reconciler.md.
use crate::llm::Llm;
use crate::md;
use crate::model::*;
use crate::parallel;
use crate::project::Project;
use crate::store::{DirtyDoc, Store};
use crate::turn::{run_turn, Trace, TurnOutput};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;

pub struct BuildReport {
    pub verdict: String,
    pub dirty_docs: usize,
    pub turns: u32,
    pub applied: usize,
    pub parked: usize,
    pub errors: usize,
    pub warnings: usize,
    pub coverage_pct: u32,
}

fn workers() -> usize {
    std::env::var("JAZYK_MAX_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(6)
        .max(1)
}

// Parse every matched document. Returns (doc -> (content hash, sections), doc -> links).
fn parse_all(proj: &Project) -> (BTreeMap<String, (String, BTreeMap<String, Section>)>, BTreeMap<String, Vec<String>>) {
    let mut parsed = BTreeMap::new();
    let mut links = BTreeMap::new();
    for f in proj.doc_files() {
        let rel = match f.strip_prefix(&proj.root) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => f.to_string_lossy().replace('\\', "/"),
        };
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        links.insert(rel.clone(), md::doc_links(&text, &rel));
        parsed.insert(rel, (hash_hex(&text), md::parse_sections(&text)));
    }
    (parsed, links)
}

// Breadth-first levels over the document link graph, starting from the root documents.
// The root level runs alone (it seeds the vocabulary); unreachable documents come last in
// path order. With no roots configured, every document is its own level, in path order.
fn schedule_levels(dirty: &[DirtyDoc], links: &BTreeMap<String, Vec<String>>, proj: &Project) -> Vec<Vec<DirtyDoc>> {
    let roots: Vec<String> = links.keys().filter(|d| proj.is_root_file(d)).cloned().collect();
    if roots.is_empty() {
        return dirty.iter().map(|d| vec![d.clone()]).collect();
    }
    let mut level_of: BTreeMap<String, usize> = BTreeMap::new();
    let mut frontier: Vec<String> = roots.clone();
    for r in &roots {
        level_of.insert(r.clone(), 0);
    }
    let mut depth = 0usize;
    while !frontier.is_empty() {
        depth += 1;
        let mut next = Vec::new();
        for doc in &frontier {
            for l in links.get(doc).map(|v| v.as_slice()).unwrap_or(&[]) {
                if links.contains_key(l) && !level_of.contains_key(l) {
                    level_of.insert(l.clone(), depth);
                    next.push(l.clone());
                }
            }
        }
        frontier = next;
    }
    let max_level = level_of.values().max().copied().unwrap_or(0);
    let mut levels: Vec<Vec<DirtyDoc>> = vec![Vec::new(); max_level + 2];
    for d in dirty {
        match level_of.get(&d.doc) {
            Some(l) => levels[*l].push(d.clone()),
            None => levels[max_level + 1].push(d.clone()),
        }
    }
    levels.retain(|l| !l.is_empty());
    levels
}

// Run one wave of work items in parallel, committing each turn's changeset as it finishes
// so later siblings see earlier commits. A failed item is retried once with fresh context,
// then parked.
fn run_wave(
    store: &Mutex<Store>,
    llm: &Llm,
    items: &[WorkItem],
    limits: &crate::project::Limits,
    lint: &crate::project::Linting,
    trace: &Trace,
) -> (usize, BTreeSet<String>, Vec<WorkItem>) {
    let applied = Mutex::new(0usize);
    let touched = Mutex::new(BTreeSet::new());
    let parked = Mutex::new(Vec::new());
    parallel::par_map(items, workers(), |_, item| {
        for attempt in 0..2 {
            let snapshot = store.lock().unwrap().clone();
            let out: TurnOutput = run_turn(llm, snapshot, item, limits, lint, trace);
            match out.failed {
                None => {
                    if out.session.staged.is_empty() {
                        trace.line(&format!("{} {}", item.task, item.target), "no mutations staged");
                        return;
                    }
                    let mut s = store.lock().unwrap();
                    let report = s.apply(out.session.staged, item, out.rounds, 0);
                    for sk in &report.skipped {
                        trace.line(&format!("{} {}", item.task, item.target), &format!("skipped at commit: {}", sk));
                    }
                    // Requirements documents render after every committed changeset, so
                    // readers and editor links stay fresh during the build.
                    if report.applied > 0 {
                        crate::docsgen::write_all(&s, &crate::gen::GenSettings::from_out(&s.out));
                    }
                    *applied.lock().unwrap() += report.applied;
                    touched.lock().unwrap().extend(report.touched_entities);
                    return;
                }
                Some(e) => {
                    trace.line(
                        &format!("{} {}", item.task, item.target),
                        &format!("turn failed (attempt {}): {}", attempt + 1, e),
                    );
                }
            }
        }
        parked.lock().unwrap().push(item.clone());
    });
    (
        applied.into_inner().unwrap(),
        touched.into_inner().unwrap(),
        parked.into_inner().unwrap(),
    )
}

// Partition review targets into connected components over shared requirements and
// relationship edges. Each component is one ordered review group.
fn review_groups(store: &Store, targets: &BTreeSet<String>) -> Vec<Vec<String>> {
    // Adjacency: two entities are neighbors when a requirement references both.
    let mut adj: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for r in store.graph.requirements.values() {
        for a in &r.entities {
            for b in &r.entities {
                if a != b {
                    adj.entry(a.clone()).or_default().insert(b.clone());
                }
            }
        }
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut groups = Vec::new();
    for t in targets {
        if seen.contains(t) {
            continue;
        }
        let mut group = Vec::new();
        let mut frontier = vec![t.clone()];
        while let Some(id) = frontier.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            if targets.contains(&id) {
                group.push(id.clone());
            }
            for n in adj.get(&id).into_iter().flatten() {
                if !seen.contains(n) {
                    frontier.push(n.clone());
                }
            }
        }
        group.sort();
        if !group.is_empty() {
            groups.push(group);
        }
    }
    groups
}

// A non-normative claim over text that still reads as obligations. Deliberately cheap
// and deterministic: `shall`, obligation verbs, access rules, or definition-list
// bullets (`- \`name\` - description`). Docs rarely say `shall`, so the word alone
// misses whole documents. Mirrors docs2/compiler/reconciler.md#coverage.
fn looks_normative(raw: &str) -> bool {
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

// Deterministic whole-graph checks. Returns (rule, subject, severity, message) findings.
fn checks(store: &Store, proj: &Project, parked: &[WorkItem]) -> Vec<(String, String, String, String)> {
    let mut f = Vec::new();
    // File-level document quality: an empty file schedules no turns and a link only
    // feeds scheduling, so neither problem ever reaches a model. These checks own them.
    for (doc, rec) in &store.docs {
        let no_content = rec.sections.values().all(|sec| {
            let skip = if sec.kind == "heading" { 1 } else { 0 };
            sec.raw.lines().skip(skip).all(|l| l.trim().is_empty())
        });
        if no_content {
            f.push((
                "empty-file".into(),
                doc.clone(),
                "warning".into(),
                format!("{} is matched by the docs glob but has no content", doc),
            ));
        }
        let mut reported: BTreeSet<String> = BTreeSet::new();
        for sec in rec.sections.values() {
            for target in md::doc_links(&sec.raw, doc) {
                if store.docs.contains_key(&target) || proj.root.join(&target).exists() || !reported.insert(target.clone()) {
                    continue;
                }
                f.push((
                    "broken-link".into(),
                    doc.clone(),
                    "warning".into(),
                    format!("{} links to {} which does not exist", doc, target),
                ));
            }
        }
    }
    // Coverage: sections that stayed unprocessed. Sections with no body under the heading
    // carry no content of their own and are skipped.
    for (doc, rec) in &store.docs {
        for (r, sec) in &rec.sections {
            let body_blank = sec.raw.lines().skip(1).all(|l| l.trim().is_empty());
            if body_blank {
                continue;
            }
            match rec.coverage.get(r) {
                None => f.push((
                    "uncovered-section".into(),
                    format!("{}#{}", doc, r),
                    "warning".into(),
                    format!("section {}#{} is unprocessed after the build", doc, r),
                )),
                Some(c) if c.state == "non-normative" && looks_normative(&sec.raw) => f.push((
                    "suspicious-non-normative".into(),
                    format!("{}#{}", doc, r),
                    "warning".into(),
                    format!("section {}#{} is marked non-normative but its text still looks normative", doc, r),
                )),
                _ => {}
            }
        }
    }
    // Entities no requirement references.
    for id in store.graph.entities.keys() {
        if store.requirements_referencing(id).is_empty() {
            f.push((
                "unused-entity".into(),
                id.clone(),
                "warning".into(),
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
        // Reachability follows typed relationships and shared requirements alike.
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
                f.push((
                    "unreachable-entity".into(),
                    id.clone(),
                    "warning".into(),
                    format!("{} is not reachable from the declared roots", id),
                ));
            }
        }
    }
    // Document-quality checks: prose problems a human can fix, surfaced where the human
    // writes (the LSP shows them inline).
    for (doc, rec) in &store.docs {
        if rec.sections.len() > proj.limits.max_doc_sections {
            f.push((
                "doc-too-large".into(),
                format!("{}#{}", doc, rec.sections.iter().find(|(_, s)| s.kind == "root").map(|(r, _)| r.clone()).unwrap_or_default()),
                "warning".into(),
                format!("{} has {} sections (cap {}); split the document", doc, rec.sections.len(), proj.limits.max_doc_sections),
            ));
        }
        for (r, sec) in &rec.sections {
            if sec.raw.len() > proj.limits.max_section_chars {
                f.push((
                    "section-too-large".into(),
                    format!("{}#{}", doc, r),
                    "warning".into(),
                    format!("{}#{} is {} chars (cap {}); split the section", doc, r, sec.raw.len(), proj.limits.max_section_chars),
                ));
            }
        }
    }
    for id in store.graph.entities.keys() {
        let n = store.requirements_referencing(id).len();
        if n > proj.limits.max_entity_requirements {
            f.push((
                "entity-too-dense".into(),
                id.clone(),
                "info".into(),
                format!(
                    "{} carries {} requirements (ceiling {}); consider splitting the topic into subsections (generation divides into parts regardless)",
                    id, n, proj.limits.max_entity_requirements
                ),
            ));
        }
    }
    // Near-identical statements on one entity: review debt made deterministic. Token-set
    // similarity catches rephrasings the requirement natural key (exact normalized text)
    // does not.
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
                by_entity.entry(e).or_default().push((rid, toks(&r.ears)));
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
                    let key = if a < b { ((*a).clone(), (*b).clone()) } else { ((*b).clone(), (*a).clone()) };
                    if !flagged.insert(key.clone()) {
                        continue;
                    }
                    if ra.source.doc == rb.source.doc {
                        // Same sentence extracted twice is a twin. Similar statements
                        // quoting different sentences are parallel enumeration items
                        // ("shall have an id" / "shall have a name"), not duplicates.
                        if ra.source.section == rb.source.section && norm(&ra.source.quote) == norm(&rb.source.quote) {
                            f.push((
                                "duplicate-requirement".into(),
                                key.0.clone(),
                                "warning".into(),
                                format!("{} and {} extract the same sentence twice; keep one", key.0, key.1),
                            ));
                        }
                    } else {
                        // Restating a fact in another document is intentional
                        // redundancy; the graph keeps both and notes the pairing.
                        f.push((
                            "duplicate-requirement".into(),
                            key.0.clone(),
                            "info".into(),
                            format!("{} and {} state the same fact in different documents; both kept", key.0, key.1),
                        ));
                    }
                }
            }
        }
    }
    // Flip detection: an entity id minted with a collision suffix while a tombstone holds
    // the base slug means a natural key was deleted and recreated across builds.
    for id in store.graph.entities.keys() {
        if let Some(pos) = id.rfind('-') {
            let (base, suffix) = id.split_at(pos);
            if suffix[1..].chars().all(|c| c.is_ascii_digit())
                && store.graph.redirects.get(base).map(|t| t.is_empty()).unwrap_or(false)
            {
                f.push((
                    "unstable-extraction".into(),
                    id.clone(),
                    "warning".into(),
                    format!("{} recreates a natural key that was deleted in an earlier build ({})", id, base),
                ));
            }
        }
    }
    // Quotes that no longer locate.
    for (rid, r) in &store.graph.requirements {
        if !store.quote_locates(&r.source.doc, &r.source.section, &r.source.quote) {
            f.push((
                "stale-provenance".into(),
                rid.clone(),
                "warning".into(),
                format!("{}'s quote no longer locates in {}#{}", rid, r.source.doc, r.source.section),
            ));
        }
    }
    // Work parked when the budget ran out.
    for p in parked {
        f.push((
            "incomplete-build".into(),
            p.target.clone(),
            "warning".into(),
            format!("work item {} {} was parked; the next build resumes it", p.task, p.target),
        ));
    }
    f
}

pub fn compile(proj: &Project, llm: &Llm, out: &Path, trace: &Trace) -> BuildReport {
    let store = Mutex::new(Store::load(out));
    let (parsed, links) = parse_all(proj);
    // Resume parked work from the previous build first.
    let previously_parked: Vec<WorkItem> = store.lock().unwrap().status.parked.clone();
    let dirty = store.lock().unwrap().sync_docs(&parsed);
    let levels = schedule_levels(&dirty, &links, proj);

    let total_dirty = dirty.len();
    trace.line(
        "reconcile",
        &format!(
            "{} dirty document(s) in {} level(s); {} parked item(s) to resume",
            total_dirty,
            levels.len(),
            previously_parked.len()
        ),
    );

    let mut turns = 0u32;
    let mut applied_total = 0usize;
    let mut touched_all: BTreeSet<String> = BTreeSet::new();
    let mut parked_all: Vec<WorkItem> = Vec::new();
    let budget_cap = proj.limits.build_turn_factor as usize * (total_dirty + previously_parked.len()).max(1) + 8;

    // Wave 1: ingest, level by level; the root level runs alone first.
    let mut wave1: Vec<Vec<WorkItem>> = Vec::new();
    if !previously_parked.is_empty() {
        wave1.push(
            previously_parked
                .iter()
                .filter(|p| p.task == "reconcile-doc")
                .cloned()
                .collect(),
        );
    }
    for level in &levels {
        wave1.push(
            level
                .iter()
                .map(|d| WorkItem {
                    task: "reconcile-doc".into(),
                    target: d.doc.clone(),
                    dirty_sections: d.dirty_sections.clone(),
                    stale_anchors: d.stale_anchors.clone(),
                })
                .filter(|w| !w.dirty_sections.is_empty() || !w.stale_anchors.is_empty())
                .collect(),
        );
    }
    for level_items in wave1 {
        if level_items.is_empty() {
            continue;
        }
        if turns as usize >= budget_cap {
            parked_all.extend(level_items);
            continue;
        }
        turns += level_items.len() as u32;
        let (applied, touched, parked) = run_wave(&store, llm, &level_items, &proj.limits, &proj.linting, trace);
        applied_total += applied;
        touched_all.extend(touched);
        parked_all.extend(parked);
    }

    // Wave 2: review entities whose fact set changed (and resumed review items).
    // Entities that share requirements or relationships form one review group: groups run
    // in parallel, entities within a group in order, so a judgment sees its neighbors'
    // merges and diagnostics.
    let mut review_targets: BTreeSet<String> = touched_all
        .iter()
        .filter(|id| store.lock().unwrap().graph.entities.contains_key(*id))
        .cloned()
        .collect();
    for p in &previously_parked {
        if p.task == "review-entity" {
            review_targets.insert(p.target.clone());
        }
    }
    let groups = review_groups(&store.lock().unwrap(), &review_targets);
    let review_count: usize = groups.iter().map(|g| g.len()).sum();
    if review_count > 0 && (turns as usize) < budget_cap {
        trace.line(
            "reconcile",
            &format!("review wave: {} entity(ies) in {} group(s)", review_count, groups.len()),
        );
        turns += review_count as u32;
        let applied = Mutex::new(0usize);
        let parked = Mutex::new(Vec::new());
        parallel::par_map(&groups, workers(), |_, group| {
            for id in group {
                // The entity may have been merged away by an earlier turn in this group.
                if !store.lock().unwrap().graph.entities.contains_key(id) {
                    continue;
                }
                let item = WorkItem {
                    task: "review-entity".into(),
                    target: id.clone(),
                    dirty_sections: vec![],
                    stale_anchors: vec![],
                };
                let (a, _t, p) = run_wave(&store, llm, std::slice::from_ref(&item), &proj.limits, &proj.linting, trace);
                *applied.lock().unwrap() += a;
                parked.lock().unwrap().extend(p);
            }
        });
        applied_total += applied.into_inner().unwrap();
        parked_all.extend(parked.into_inner().unwrap());
    } else if review_count > 0 {
        for g in groups {
            for id in g {
                parked_all.push(WorkItem { task: "review-entity".into(), target: id, dirty_sections: vec![], stale_anchors: vec![] });
            }
        }
    }

    // Deterministic cleanup.
    {
        let mut s = store.lock().unwrap();
        let gc_actions = s.gc();
        if !gc_actions.is_empty() {
            trace.line("reconcile", &format!("gc: {}", gc_actions.join("; ")));
        }
    }

    // One bounded fix-up pass: sections that stayed unprocessed re-enqueue their document
    // once, so a partially covered document is not silently left behind.
    let fixup: Vec<WorkItem> = {
        let s = store.lock().unwrap();
        let parked_docs: BTreeSet<&String> = parked_all.iter().map(|p| &p.target).collect();
        s.docs
            .iter()
            .filter(|(doc, _)| !parked_docs.contains(doc))
            .filter_map(|(doc, rec)| {
                let uncovered: Vec<String> = rec
                    .sections
                    .iter()
                    .filter(|(r, sec)| {
                        !rec.coverage.contains_key(*r)
                            && !sec.raw.lines().skip(1).all(|l| l.trim().is_empty())
                    })
                    .map(|(r, _)| r.clone())
                    .collect();
                // Also re-enqueue stale anchors: requirements whose quote no longer
                // locates in this document, left behind by a failed turn.
                let mut stale: Vec<String> = Vec::new();
                let mut stale_sections: Vec<String> = Vec::new();
                for (rid, r) in &s.graph.requirements {
                    if &r.source.doc == doc && !s.quote_locates(&r.source.doc, &r.source.section, &r.source.quote) {
                        stale.push(rid.clone());
                        if !stale_sections.contains(&r.source.section) && rec.sections.contains_key(&r.source.section) {
                            stale_sections.push(r.source.section.clone());
                        }
                    }
                }
                let mut dirty = uncovered;
                for sec in stale_sections {
                    if !dirty.contains(&sec) {
                        dirty.push(sec);
                    }
                }
                if dirty.is_empty() && stale.is_empty() {
                    None
                } else {
                    Some(WorkItem {
                        task: "reconcile-doc".into(),
                        target: doc.clone(),
                        dirty_sections: dirty,
                        stale_anchors: stale,
                    })
                }
            })
            .collect()
    };
    if !fixup.is_empty() && (turns as usize) < budget_cap {
        trace.line("reconcile", &format!("fix-up pass: {} document(s) with uncovered sections or stale anchors", fixup.len()));
        turns += fixup.len() as u32;
        let (applied, _touched, parked) = run_wave(&store, llm, &fixup, &proj.limits, &proj.linting, trace);
        applied_total += applied;
        parked_all.extend(parked);
        let mut s = store.lock().unwrap();
        s.gc();
    }

    // Checks and status.
    let mut s = store.into_inner().unwrap();
    let findings = checks(&s, proj, &parked_all);
    s.reconcile_check_diags(findings);

    // Status and verdict.
    s.status.parked = parked_all.clone();
    s.status.verdict = if parked_all.is_empty() { "converged".into() } else { "incomplete".into() };
    let n = crate::docsgen::write_all(&s, &crate::gen::GenSettings::resolve(proj, &s.out));
    if n > 0 {
        trace.line("reconcile", &format!("docsgen: {} requirements document(s)", n));
    }
    s.status.spent.tokens = crate::llm::tokens_spent();
    s.save_status();

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
            if sec.raw.lines().skip(1).all(|l| l.trim().is_empty()) {
                continue;
            }
            total_secs += 1;
            if rec.coverage.contains_key(r) {
                covered_secs += 1;
            }
        }
    }
    BuildReport {
        verdict: s.status.verdict.clone(),
        dirty_docs: total_dirty,
        turns,
        applied: applied_total,
        parked: parked_all.len(),
        errors,
        warnings,
        coverage_pct: if total_secs == 0 { 100 } else { (covered_secs * 100 / total_secs) as u32 },
    }
}

#[cfg(test)]
mod tests {
    use super::looks_normative;

    #[test]
    fn normative_signals_catch_prose_without_shall() {
        // The example-erp user.md failure: obligation verbs and access rules, no `shall`.
        assert!(looks_normative("The user management system handles user accounts and authentication.\n"));
        assert!(looks_normative("Login operation can be performed by unauthenticated.\n"));
        // Definition-list bullets: an operations or properties catalog.
        assert!(looks_normative("# Operations\n- `addProduct` - adds a new product to the inventory\n"));
        assert!(looks_normative("Sections shall be covered.\n"));
    }

    #[test]
    fn navigation_and_changelog_prose_stays_quiet() {
        assert!(!looks_normative("See the [frontend documentation](./frontend.md) for more information.\n"));
        assert!(!looks_normative("# Changelog\n- 1.2: fixed typos in the intro\n"));
    }
}
