// The reconciler: compares the documents (desired state) against the graph (observed
// state) and schedules turns until they agree. Deterministic; the model never decides
// what is stale or what runs next. Mirrors docs/compiler/reconciler.md.
use crate::llm::Llm;
use crate::md;
use crate::model::*;
use crate::parallel;
use crate::project::Project;
use crate::store::{DirtyDoc, Store};
use crate::turn::Trace;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
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

// Wave counter for the trace, reset per build so a run's waves number from one.
static WAVE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn workers() -> usize {
    std::env::var("JAZYK_MAX_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(6)
        .max(1)
}

// Parse every matched document. Returns (doc -> (content hash, sections), doc -> links).
pub fn parse_all(proj: &Project) -> (BTreeMap<String, (String, BTreeMap<String, Section>)>, BTreeMap<String, Vec<String>>) {
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
pub fn schedule_levels(dirty: &[DirtyDoc], links: &BTreeMap<String, Vec<String>>, proj: &Project) -> Vec<Vec<DirtyDoc>> {
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

// Run one wave of work items in parallel, each as one ACP worker session. The
// injected serving commits each turn's changeset as it finishes, so later siblings
// see earlier commits; the shared store reloads from disk when the wave ends. A
// failed item is retried once with a fresh session, then parked.
fn run_wave(
    store: &Mutex<Store>,
    runner: &crate::acp::runner::AcpRunner,
    items: &[WorkItem],
    parsed: &BTreeMap<String, (String, BTreeMap<String, crate::model::Section>)>,
    out: &Path,
    trace: &Trace,
) -> (usize, BTreeSet<String>, BTreeSet<String>, Vec<WorkItem>) {
    // What is queued, before any turn starts: the frontends mark these targets as
    // waiting (docs/compiler/turns.md#trace-events).
    if !items.is_empty() {
        let wave = WAVE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        trace.event(crate::turn::TraceEvent::WaveStart {
            wave,
            task: items[0].task.clone(),
            items: items.iter().map(|i| i.target.clone()).collect(),
        });
    }
    let applied = Mutex::new(0usize);
    let touched = Mutex::new(BTreeSet::new());
    let changed = Mutex::new(BTreeSet::new());
    let parked = Mutex::new(Vec::new());
    parallel::par_map(items, workers(), |_, item| {
        // Best-effort cancellation: an unstarted item parks; the next build resumes it.
        if trace.is_cancelled() {
            parked.lock().unwrap().push(item.clone());
            return;
        }
        for attempt in 0..2 {
            let report = runner.run_item(item, trace);
            match report.failed {
                None => {
                    if report.applied == 0 {
                        trace.line(&format!("{} {}", item.task, item.target), "no mutations staged");
                    }
                    *applied.lock().unwrap() += report.applied;
                    touched.lock().unwrap().extend(report.touched);
                    changed.lock().unwrap().extend(report.changed);
                    return;
                }
                Some(e) => {
                    trace.event(crate::turn::TraceEvent::TurnFailed {
                        label: format!("{} {}", item.task, item.target),
                        attempt: attempt + 1,
                        error: e,
                    });
                }
            }
        }
        parked.lock().unwrap().push(item.clone());
    });
    // The wave's commits live on disk; the in-memory store the scheduling reads
    // catches up here, section trees synced the same way the build started.
    {
        let mut s = store.lock().unwrap();
        *s = Store::load(out);
        s.sync_docs(parsed);
    }
    (
        applied.into_inner().unwrap(),
        touched.into_inner().unwrap(),
        changed.into_inner().unwrap(),
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

// Deterministic whole-graph checks. Returns (rule, subject, severity, message) findings.
// Pinned-fact drift: a code-span literal in a bound requirement's statement that
// none of its bound files mention. The docs say `us-east-1` and the code never does:
// one of them is wrong, and no model is needed to notice. The finding carries its
// question, and an answered question is never re-asked (the prompt merge in
// reconcile_check_diags). Mirrors docs/compiler/compilation.md#waves.
fn drift_checks(
    store: &Store,
    proj: &Project,
) -> Vec<(String, String, String, String, Option<crate::model::DiagnosticPrompt>)> {
    use crate::model::{DiagnosticPrompt, PromptOption};
    let ledger = crate::gen::Ledger::load(&proj.out);
    if ledger.requirements.is_empty() {
        return Vec::new();
    }
    let gs = crate::gen::GenSettings::resolve(proj);
    let mut findings = Vec::new();
    for (rid, row) in &ledger.requirements {
        let Some(req) = store.graph.requirements.get(rid) else { continue };
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
        for lit in pinned_literals(&req.ears) {
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
                rid.clone(),
                "warning".to_string(),
                format!("{} pins `{}`; bound file(s) {} never mention it", rid, lit, files.join(", ")),
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
fn pinned_literals(ears: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut rest = ears;
    while let Some(b) = rest.find('`') {
        let after = &rest[b + 1..];
        let Some(e) = after.find('`') else { break };
        let tok = after[..e].trim();
        rest = &after[e + 1..];
        if tok.len() >= 4
            && tok.len() <= 80
            && !tok.contains(' ')
            && tok.chars().any(|c| c.is_ascii_digit() || ['.', '/', '-', ':', '_'].contains(&c))
        {
            v.push(tok.to_string());
        }
    }
    v.sort();
    v.dedup();
    v
}

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
    // Decompiled documents nobody has touched since the machine wrote them: the
    // statements are provisional until a human edit moves the hash.
    // Mirrors docs/consumers/decompile.md#ratification.
    for doc in crate::decompile::unratified(store) {
        f.push((
            "unratified".into(),
            doc.clone(),
            "info".into(),
            format!("{} is a decompiled draft nobody has ratified; review it by editing it, even a one-line change clears this", doc),
        ));
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
    let refused = |e: String| {
        trace.line("reconcile", &format!("build refused: {}", e));
        BuildReport {
            verdict: "incomplete".into(),
            dirty_docs: 0,
            turns: 0,
            applied: 0,
            parked: 0,
            errors: 1,
            warnings: 0,
            coverage_pct: 0,
        }
    };
    // The control plane's build contract: one coarse lease for the run, refused while
    // an agent is mid-task, and the run itself is an approval in manual mode.
    // Mirrors docs/compiler/control-plane.md.
    let _build = match crate::control::begin_internal_build(proj, out, "compile") {
        Ok(g) => g,
        Err(e) => return refused(e),
    };
    // Every turn runs as an ACP worker session against the configured agent; the
    // agent process lives for the run. Mirrors docs/frontends/acp.md#worker-sessions.
    let runner = match crate::acp::runner::AcpRunner::start(proj, llm, out) {
        Ok(r) => r,
        Err(e) => return refused(e),
    };
    runner.set_build_token(Some(format!("internal-{}", std::process::id())));
    trace.line("reconcile", &format!("agent: {}", runner.agent().name));
    WAVE.store(0, std::sync::atomic::Ordering::Relaxed);
    let store = Mutex::new(Store::load(out));
    let gs = crate::gen::GenSettings::resolve(proj);
    let _ = &gs;
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
    let mut changed_all: BTreeSet<String> = BTreeSet::new();
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
        if turns as usize >= budget_cap || trace.is_cancelled() {
            parked_all.extend(level_items);
            continue;
        }
        turns += level_items.len() as u32;
        let (applied, touched, changed, parked) = run_wave(&store, &runner, &level_items, &parsed, out, trace);
        applied_total += applied;
        touched_all.extend(touched);
        changed_all.extend(changed);
        parked_all.extend(parked);
    }

    // Deterministic cleanup before the fix-up sweep reads coverage.
    {
        let mut s = store.lock().unwrap();
        let gc_actions = s.gc();
        if !gc_actions.is_empty() {
            trace.line("reconcile", &format!("gc: {}", gc_actions.join("; ")));
        }
        // Settle stranded diagnostics now, so this build's pair wave picks up the
        // re-enqueued survivors instead of leaving them to the next one.
        let settled = s.settle_dangling_diags();
        if !settled.is_empty() {
            trace.line("reconcile", &format!("settle: {}", settled.join("; ")));
        }
    }

    // One bounded fix-up pass, BEFORE judgment: sections that stayed unprocessed
    // re-enqueue their document once, so a partially covered document is not silently
    // left behind. Coverage outranks review when the budget is tight; a fix-up that no
    // longer fits the budget parks instead of vanishing, so the verdict stays honest.
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
                        let skip = if sec.kind == "heading" { 1 } else { 0 };
                        !rec.coverage.contains_key(*r)
                            && !sec.raw.lines().skip(skip).all(|l| l.trim().is_empty())
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
    if !fixup.is_empty() {
        if (turns as usize) >= budget_cap || trace.is_cancelled() {
            parked_all.extend(fixup);
        } else {
            trace.line("reconcile", &format!("fix-up pass: {} document(s) with uncovered sections or stale anchors", fixup.len()));
            turns += fixup.len() as u32;
            let (applied, touched, changed, parked) = run_wave(&store, &runner, &fixup, &parsed, out, trace);
            applied_total += applied;
            touched_all.extend(touched);
            changed_all.extend(changed);
            parked_all.extend(parked);
            let mut s = store.lock().unwrap();
            s.gc();
        }
    }

    // Pair-review wave: dirtiness propagates from sections to requirements. Every
    // requirement the ingest and fix-up commits created or revised is re-judged
    // against its computed neighbors and sticky partners. A changed requirement
    // with neither schedules nothing. Mirrors docs/compiler/compilation.md#waves.
    let mut pair_targets: BTreeSet<String> = changed_all.clone();
    for p in &previously_parked {
        if p.task == "review-requirement" {
            pair_targets.insert(p.target.clone());
        }
    }
    // Reviews owed from earlier builds or other consumers: the pending block is the
    // durable form of changed_all (docs/compiler/reconciler.md#the-task-queue).
    pair_targets.extend(store.lock().unwrap().status.pending.requirements.iter().cloned());
    let pair_items: Vec<WorkItem> = {
        let s = store.lock().unwrap();
        pair_targets
            .iter()
            .filter(|rid| s.pair_review_due(rid))
            // A pair scheduled from both ends runs once: when two targets are each
            // other's only neighbor, the smaller id carries the task and completion
            // mirrors to the other.
            .filter(|rid| {
                let nbrs = s.pair_review_neighbors(rid);
                !(nbrs.len() == 1
                    && nbrs[0].as_str() < rid.as_str()
                    && pair_targets.contains(&nbrs[0])
                    && s.pair_review_neighbors(&nbrs[0]).iter().any(|x| x == *rid))
            })
            .map(|rid| WorkItem {
                task: "review-requirement".into(),
                target: rid.clone(),
                dirty_sections: vec![],
                stale_anchors: vec![],
            })
            .collect()
    };
    if !pair_items.is_empty() {
        if (turns as usize) + pair_items.len() > budget_cap || trace.is_cancelled() {
            parked_all.extend(pair_items);
        } else {
            trace.line("reconcile", &format!("pair review: {} changed requirement(s)", pair_items.len()));
            turns += pair_items.len() as u32;
            let (applied, touched, _changed, parked) = run_wave(&store, &runner, &pair_items, &parsed, out, trace);
            applied_total += applied;
            touched_all.extend(touched);
            // The serving completes each pair review (and its mirror) at finish;
            // a parked one keeps its pending debt.
            parked_all.extend(parked);
            let mut s = store.lock().unwrap();
            s.gc();
        }
    }

    // Wave 2: review entities whose fact set changed (and resumed review items).
    // Entities that share requirements or relationships form one review group: groups run
    // in parallel, entities within a group in order, so a judgment sees its neighbors'
    // merges and diagnostics. Whole groups run while they fit the budget; the rest parks
    // and the next build resumes it.
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
    // Reviews owed from earlier builds or other consumers.
    {
        let s = store.lock().unwrap();
        for id in &s.status.pending.entities {
            if s.graph.entities.contains_key(id) {
                review_targets.insert(id.clone());
            }
        }
    }
    let groups = review_groups(&store.lock().unwrap(), &review_targets);
    let mut run_groups: Vec<Vec<String>> = Vec::new();
    for g in groups {
        if (turns as usize) + g.len() <= budget_cap && !trace.is_cancelled() {
            turns += g.len() as u32;
            run_groups.push(g);
        } else {
            for id in g {
                parked_all.push(WorkItem { task: "review-entity".into(), target: id, dirty_sections: vec![], stale_anchors: vec![] });
            }
        }
    }
    if !run_groups.is_empty() {
        trace.line(
            "reconcile",
            &format!(
                "review wave: {} entity(ies) in {} group(s)",
                run_groups.iter().map(|g| g.len()).sum::<usize>(),
                run_groups.len()
            ),
        );
        let applied = Mutex::new(0usize);
        let parked = Mutex::new(Vec::new());
        parallel::par_map(&run_groups, workers(), |_, group| {
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
                let (a, _t, _c, p) = run_wave(&store, &runner, std::slice::from_ref(&item), &parsed, out, trace);
                *applied.lock().unwrap() += a;
                // The serving completes the review at finish; a parked one stays owed.
                parked.lock().unwrap().extend(p);
            }
        });
        applied_total += applied.into_inner().unwrap();
        parked_all.extend(parked.into_inner().unwrap());
        let mut s = store.lock().unwrap();
        s.gc();
    }

    // Checks and status.
    let mut s = store.into_inner().unwrap();
    let mut report = finalize(&mut s, proj, &parked_all, trace);
    report.dirty_docs = total_dirty;
    report.turns = turns;
    report.applied = applied_total;
    report
}

// The deterministic tail of a build: checks, verdict, docsgen, status. No model
// anywhere, so whichever consumer empties the task queue runs it, the internal loop
// and the MCP serving alike. Mirrors docs/compiler/reconciler.md#the-task-queue.
pub fn finalize(s: &mut Store, proj: &Project, parked_all: &[WorkItem], trace: &Trace) -> BuildReport {
    // Level-triggered deletion propagation: settle open judged diagnostics whose
    // subjects left the graph, re-enqueueing survivors for review
    // (docs/compiler/compilation.md#waves).
    let settled = s.settle_dangling_diags();
    if !settled.is_empty() {
        trace.line("reconcile", &format!("settle: {}", settled.join("; ")));
    }
    let mut findings: Vec<(String, String, String, String, Option<crate::model::DiagnosticPrompt>)> =
        checks(s, proj, parked_all).into_iter().map(|(a, b, c, d)| (a, b, c, d, None)).collect();
    findings.extend(drift_checks(s, proj));
    s.reconcile_check_diags(findings);

    // Status and verdict. Coverage is part of the termination criterion
    // (docs/compiler/compilation.md#coverage): a section the build never processed is
    // work still open, whether or not its turn parked. A turn that exhausts its round
    // budget commits what it staged and returns no failure, so parked items alone would
    // report a build that stopped early as converged. Pending reviews are open work the
    // same way.
    // Same filter the uncovered-section check uses: a heading with no body of its own
    // carries no content to process.
    let unprocessed = s
        .docs
        .values()
        .flat_map(|rec| {
            rec.sections.iter().filter(|(r, sec)| {
                !rec.coverage.contains_key(*r) && !sec.raw.lines().skip(1).all(|l| l.trim().is_empty())
            })
        })
        .count();
    // A pending review whose target no longer exists is complete by definition, and so
    // is a pair review with nothing tying it to a judgment: no computed neighbors and
    // no open judged diagnostic naming it (docs/compiler/compilation.md#waves).
    let (exists_e, exists_r): (Vec<String>, Vec<String>) = (
        s.status.pending.entities.iter().filter(|e| s.graph.entities.contains_key(*e)).cloned().collect(),
        s.status
            .pending
            .requirements
            .iter()
            .filter(|r| s.pair_review_due(r))
            .cloned()
            .collect(),
    );
    s.status.pending.entities = exists_e;
    s.status.pending.requirements = exists_r;
    s.status.parked = parked_all.to_vec();
    s.status.verdict = if parked_all.is_empty() && unprocessed == 0 && s.status.pending.is_empty() {
        "converged".into()
    } else {
        "incomplete".into()
    };
    let n = crate::docsgen::write_all(s, &crate::gen::GenSettings::resolve(proj));
    if n > 0 {
        trace.line("reconcile", &format!("docsgen: {} requirements document(s)", n));
    }
    s.status.spent.tokens = crate::llm::tokens_spent();
    // The verdict never travels alone: open diagnostic counts ride beside it
    // (docs/compiler/compilation.md#convergence).
    s.status.diagnostics = s.open_diag_counts();
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
        dirty_docs: 0,
        turns: 0,
        applied: 0,
        parked: parked_all.len(),
        errors,
        warnings,
        coverage_pct: if total_secs == 0 { 100 } else { (covered_secs * 100 / total_secs) as u32 },
    }
}

#[cfg(test)]
mod tests {
    use super::{checks, drift_checks, looks_normative, pinned_literals};
    use crate::model::{hash_hex, DocRecord, Requirement, SourceRef};
    use crate::project::Project;
    use crate::store::Store;
    use std::collections::BTreeMap;

    fn seed_doc(store: &mut Store, doc: &str, text: &str) {
        store.docs.insert(
            doc.to_string(),
            DocRecord { content_hash: hash_hex(text), sections: crate::md::parse_sections(text), coverage: BTreeMap::new() },
        );
    }

    // Pinned literals: code-span tokens that read as values, not words.
    #[test]
    fn pinned_literals_picks_values_and_skips_words() {
        let lits = pinned_literals(
            "The gateway shall log to `/var/log/gw.log` using model `us.claude-4` while the `username` field stays unique and `--verbose` widens it.",
        );
        assert!(lits.contains(&"/var/log/gw.log".to_string()), "{:?}", lits);
        assert!(lits.contains(&"us.claude-4".to_string()), "{:?}", lits);
        assert!(lits.contains(&"--verbose".to_string()), "{:?}", lits);
        assert!(!lits.iter().any(|l| l == "username"), "a plain word is not pinned: {:?}", lits);
    }

    // The drift check: a pinned literal none of the bound files mention becomes a
    // prompted warning; a mentioned one stays silent.
    // Mirrors docs/compiler/compilation.md#waves.
    #[test]
    fn drift_check_flags_missing_literals_with_a_question() {
        use crate::gen::{Ledger, ReqRow, RowHashes, TestRef};
        let _ = Ledger::path; // path is the save location; save() writes it
        let dir = std::env::temp_dir().join(format!("jazyk-drift-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("product")).unwrap();
        std::fs::write(dir.join("jazyk.toml"), "[docs]\nglob = [\"docs/**/*.md\"]\n\n[gen]\ndeliverable = \"./product\"\n").unwrap();
        std::fs::write(dir.join("product/gw.rs"), "fn log() { /* writes to /tmp/other.log */ }\n").unwrap();
        let proj = Project::load(&dir);
        let mut store = Store { out: proj.out.clone(), ..Default::default() };
        store.graph.requirements.insert(
            "req:gw-1".into(),
            req("The gateway shall log to `/var/log/gw.log`.", "ent:gw", "docs/gw.md", "/gw", "logs"),
        );
        store.graph.requirements.insert(
            "req:gw-2".into(),
            req("The gateway shall keep `/tmp/other.log` rotating.", "ent:gw", "docs/gw.md", "/gw", "rotates"),
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
        assert_eq!(f.len(), 1, "only the missing literal fires: {:?}", f.iter().map(|x| &x.1).collect::<Vec<_>>());
        let (rule, subject, sev, msg, prompt) = &f[0];
        assert_eq!(rule, "pinned-fact-drift");
        assert_eq!(subject, "req:gw-1");
        assert_eq!(sev, "warning");
        assert!(msg.contains("/var/log/gw.log"), "{}", msg);
        let p = prompt.as_ref().expect("the finding carries its question");
        assert_eq!(p.options.len(), 2);
        assert!(p.freeform);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn req(ears: &str, entity: &str, doc: &str, section: &str, quote: &str) -> Requirement {
        Requirement {
            ears: ears.into(),
            entities: vec![entity.into()],
            edges: Vec::new(),
            source: SourceRef { doc: doc.into(), section: section.into(), quote: quote.into() },
            confidence: None,
            reasoning: None,
            created: None,
            updated: None,
        }
    }

    fn rules_for<'a>(f: &'a [(String, String, String, String)], rule: &str) -> Vec<&'a (String, String, String, String)> {
        f.iter().filter(|(r, _, _, _)| r == rule).collect()
    }

    #[test]
    fn empty_file_flagged_with_zero_llm_calls() {
        let mut s = Store::default();
        seed_doc(&mut s, "empty.md", "");
        seed_doc(&mut s, "blank.md", "\n\n  \n");
        seed_doc(&mut s, "full.md", "# Full\nThe system shall respond.\n");
        let f = checks(&s, &Project::default(), &[]);
        let hits = rules_for(&f, "empty-file");
        assert_eq!(hits.len(), 2, "{:?}", hits);
        assert!(hits.iter().all(|(_, subj, sev, _)| sev == "warning" && subj != "full.md"));
    }

    #[test]
    fn broken_link_flagged_only_for_missing_md_targets() {
        let mut s = Store::default();
        seed_doc(&mut s, "a.md", "# A\nSee [b](./b.md) and [gone](./no-such-doc-xyz.md).\n");
        seed_doc(&mut s, "b.md", "# B\ncontent\n");
        let f = checks(&s, &Project::default(), &[]);
        let hits = rules_for(&f, "broken-link");
        assert_eq!(hits.len(), 1, "{:?}", hits);
        assert_eq!(hits[0].1, "a.md");
        assert_eq!(hits[0].2, "warning");
        assert!(hits[0].3.contains("no-such-doc-xyz.md"));
    }

    #[test]
    fn duplicate_requirement_splits_warning_and_info() {
        let mut s = Store::default();
        // Same sentence extracted twice: a twin, warning.
        s.graph.requirements.insert(
            "req:a-1".into(),
            req("The system shall archive completed orders.", "ent:one", "a.md", "/a", "archives completed orders"),
        );
        s.graph.requirements.insert(
            "req:a-2".into(),
            req("The system shall archive completed orders.", "ent:one", "a.md", "/a", "archives completed orders"),
        );
        // The same fact restated in another document: intentional redundancy, info.
        s.graph.requirements.insert(
            "req:b-1".into(),
            req("The store shall mint every id at creation.", "ent:two", "a.md", "/a", "mints every id at creation"),
        );
        s.graph.requirements.insert(
            "req:b-2".into(),
            req("The store shall mint every id at creation.", "ent:two", "b.md", "/b", "the store mints every id"),
        );
        // Parallel enumeration items in one document: similar statements, different
        // sentences, not duplicates.
        s.graph.requirements.insert(
            "req:c-1".into(),
            req("The record shall have an id field.", "ent:three", "c.md", "/c", "- `id` - the identifier"),
        );
        s.graph.requirements.insert(
            "req:c-2".into(),
            req("The record shall have a name field.", "ent:three", "c.md", "/c", "- `name` - the display name"),
        );
        let f = checks(&s, &Project::default(), &[]);
        let hits = rules_for(&f, "duplicate-requirement");
        assert_eq!(hits.len(), 2, "{:?}", hits);
        assert!(hits.iter().any(|(_, subj, sev, msg)| subj == "req:a-1" && sev == "warning" && msg.contains("keep one")));
        assert!(hits.iter().any(|(_, subj, sev, msg)| subj == "req:b-1" && sev == "info" && msg.contains("both kept")));
    }

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
        // A lead-in-only body defers its items to child sections; nothing to extract here.
        assert!(!looks_normative("# Operations\n\nThe user management system supports the following operations:\n"));
    }
}
