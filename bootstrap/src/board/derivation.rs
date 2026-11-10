// Deriving the board: target existence, link levels, the parked and failed entries,
// leases, and Board::derive and Board::compute.
// Mirrors docs/compiler/reconciler.md#goal-derivation.
use super::*;

pub(super) fn target_exists(store: &Store, target: &str) -> bool {
    if let Some((a, b)) = goals::pair_members(target) {
        return target_exists(store, a) && target_exists(store, b);
    }
    // A scope root exists while the scope holds a parentless entity.
    if let Some(scope) = scope_target(target) {
        return !scope_root(store, scope).is_empty();
    }
    if let Some((doc, sec)) = split_section_ref(target) {
        return store
            .docs
            .get(&doc)
            .is_some_and(|r| r.sections.contains_key(&sec));
    }
    if target.contains(':') {
        return store.graph.entities.contains_key(target)
            || store.graph.requirements.contains_key(target)
            || store.graph.views.contains_key(target)
            || store.graph.diagnostics.contains_key(target);
    }
    store.docs.contains_key(target)
}

pub(super) fn is_open(g: &Goal) -> bool {
    matches!(g.state, GoalState::Open | GoalState::Parked)
}

// The document a goal's target lives in, when it has one.
pub fn target_doc(target: &str) -> Option<String> {
    if let Some((doc, _)) = split_section_ref(target) {
        return Some(doc);
    }
    if crate::md::is_document_target(target) {
        return Some(target.to_string());
    }
    None
}

// Breadth-first document levels from the roots over the link graph in the stored
// sections (derive::doc_levels_from does the walk). With no roots, every document is
// its own level in path order. Mirrors docs/compiler/reconciler.md#link-levels.
pub fn doc_levels(store: &Store, proj: &Project) -> BTreeMap<String, usize> {
    let roots: Vec<String> = store
        .docs
        .keys()
        .filter(|d| proj.is_root_file(d))
        .cloned()
        .collect();
    let mut levels = crate::derive::doc_levels_from(store, &roots);
    let last = levels.values().copied().max().unwrap_or(0) + 1;
    for (doc, rec) in &store.docs {
        if rec.sections.values().any(|s| s.kind == "blob") {
            levels.insert(doc.clone(), last);
        }
    }
    levels
}

// Whether a failed goal's entry still covers the goal as re-derived: the same change,
// or no newer cause; for an abstract-entity goal over a limit, a count that has not
// grown by a fifth since the failure (growth alone is not a new subject).
// Mirrors docs/compiler/reconciler.md#parked-and-failed.
pub(crate) fn failure_stands(failed: &Goal, derived: &Goal) -> bool {
    if derived.change == failed.change {
        return true;
    }
    if derived.kind == "abstract-entity" {
        let within = |then: Option<u64>, now: Option<u64>| match (then, now) {
            (Some(then), Some(now)) => Some(now * 5 < then * 6),
            (None, None) => None,
            _ => Some(false),
        };
        let bound = |g: &Goal| {
            (
                g.change["limits"][0]["soft"].clone(),
                g.change["limits"][0]["hard"].clone(),
            )
        };
        let caps = within(
            failed.change["limits"][0]["count"].as_u64(),
            derived.change["limits"][0]["count"].as_u64(),
        )
        .map(|w| w && bound(failed) == bound(derived));
        // The fan-out variant: the coupling candidates move with every requirement
        // on the level, so only the child count and the limits in force (a bump)
        // reopen it. A level declared flat stays failed until it changes.
        let fan_out = within(
            failed.change["fan_out"].as_u64(),
            derived.change["fan_out"].as_u64(),
        )
        .map(|w| w && failed.change["limit"] == derived.change["limit"]);
        match (caps, fan_out) {
            (Some(true), None | Some(true)) | (None, Some(true)) => return true,
            _ => {}
        }
    }
    derived.cause.as_ref().map(|c| c.generation) <= failed.cause.as_ref().map(|c| c.generation)
}

// The state a stored parked entry resumes in. A goal parked by flip detection waits
// on the ruling and is never batched, whatever state the entry was written with: a
// parked goal resumes first, and a session cannot resolve one whose cause is the
// ruling owed (docs/compiler/reconciler.md#flip-detection).
fn parked_state(p: &Goal) -> GoalState {
    let Some(subject) = flip_subject(p) else {
        return GoalState::Parked;
    };
    GoalState::Blocked {
        on: format!("human: unstable-derivation ruling on {}", subject),
    }
}

// The node a flip-parked entry waits on a ruling for; None for any other park.
pub(crate) fn flip_subject(p: &Goal) -> Option<String> {
    if !p.cause.as_ref().is_some_and(|c| c.via == "flip-detection") {
        return None;
    }
    Some(
        p.change["reparent"]
            .as_str()
            .or_else(|| p.change["subject"].as_str())
            .unwrap_or(&p.target)
            .to_string(),
    )
}

// Whether the unstable-derivation ruling on a subject is still owed: an open
// diagnostic naming it carries no answer yet.
fn ruling_owed(store: &Store, subject: &str) -> bool {
    store.graph.diagnostics.values().any(|d| {
        d.lifecycle == "open"
            && d.rule == "unstable-derivation"
            && d.answer.is_none()
            && d.subjects.iter().any(|s| s == subject)
    })
}

// `b<generation>-<n>`, the id a batch lease is filed under.
fn is_batch_id(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('b') else {
        return false;
    };
    let Some((g, n)) = rest.split_once('-') else {
        return false;
    };
    !g.is_empty()
        && !n.is_empty()
        && g.bytes().all(|c| c.is_ascii_digit())
        && n.bytes().all(|c| c.is_ascii_digit())
}

// What a board depends on besides the files: the project's resolved settings.
#[cfg(target_arch = "wasm32")]
fn memo_key(proj: &Project, out: &Path) -> String {
    format!(
        "{:?}|{:?}|{:?}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}|{}|{:?}|{:?}",
        proj.root,
        out,
        proj.set_id,
        proj.config_hash,
        proj.config_error,
        proj.docs_glob,
        proj.roots,
        proj.gen_deliverable,
        proj.gen_code,
        proj.workflow.compile,
        proj.workflow.generate,
        proj.workflow.worker,
        proj.explicit_files,
        proj.set_docs,
    )
}

impl Board {
    // Derive the board from a store synced against the documents. Every process
    // computes the same board from the same disk state.
    pub fn derive(store: &Store, proj: &Project, control: &Control) -> Board {
        // The per-goal scans answer from indices built once for this derivation.
        // Mirrors docs/compiler/graph.md#derived-data.
        let _scope = crate::store::ReadScope::enter(store);
        let gen = GenSettings::resolve(proj);
        // The pair plan is computed once: its open goals stand in for the kind's
        // derivation, its waiting pairs ride on the board.
        let pairs = goals::pair_plan(store);
        let mut goals: Vec<Goal> = REGISTRY
            .iter()
            .filter(|k| k.kind() != "rejudge-pair")
            .flat_map(|k| k.derive_goals(store, &gen))
            .chain(pairs.open)
            .collect();
        goals.sort_by(|a, b| a.id.cmp(&b.id));
        goals.dedup_by(|a, b| a.id == b.id);

        // Parked and failed goals keep their identity (the change) and their state.
        let mut dropped_parked = Vec::new();
        let mut parked_reasons = BTreeMap::new();
        let mut reopened = Vec::new();
        let mut dropped_failed = Vec::new();
        // Goal id to its position, kept as parked and failed goals join the list.
        let mut at: std::collections::HashMap<String, usize> = goals
            .iter()
            .enumerate()
            .map(|(i, g)| (g.id.clone(), i))
            .collect();
        for entry in &store.status.parked {
            let p = &entry.goal;
            let mut kept = true;
            // A pair parked by flip detection stays parked while its ruling is owed,
            // whatever change the goal derives with now (the parked change names the
            // flip, never the goal's own records); answering the ruling clears it.
            // Mirrors docs/compiler/reconciler.md#flip-detection.
            if let Some(subject) = flip_subject(p) {
                if !ruling_owed(store, &subject) {
                    dropped_parked.push(p.id.clone());
                    continue;
                }
                match at.get(&p.id).map(|&i| &mut goals[i]) {
                    Some(g) => g.state = parked_state(p),
                    None if target_exists(store, &p.target) => {
                        let mut g = p.clone();
                        g.state = parked_state(p);
                        at.insert(g.id.clone(), goals.len());
                        goals.push(g);
                    }
                    None => {
                        dropped_parked.push(p.id.clone());
                        continue;
                    }
                }
                if let Some(r) = &entry.reason {
                    parked_reasons.insert(p.id.clone(), r.clone());
                }
                continue;
            }
            match at.get(&p.id).map(|&i| &mut goals[i]) {
                Some(g) => {
                    if g.change == p.change || p.change.is_null() {
                        if matches!(g.state, GoalState::Open) {
                            g.state = parked_state(p);
                        }
                    } else {
                        kept = false;
                    }
                }
                None => {
                    // A ledger goal is the ledger's to derive: a parked verify goal
                    // whose row reads verified has nothing left to resume (the slides
                    // example carried two such goals across every run).
                    let ledger_kind = matches!(p.kind.as_str(), "bind" | "generate" | "verify");
                    if target_exists(store, &p.target)
                        && !goals::blocked_on_human(&p.kind)
                        && !ledger_kind
                    {
                        let mut g = p.clone();
                        g.state = parked_state(p);
                        at.insert(g.id.clone(), goals.len());
                        goals.push(g);
                    } else {
                        kept = false;
                    }
                }
            }
            if !kept {
                dropped_parked.push(p.id.clone());
            } else if let Some(r) = &entry.reason {
                parked_reasons.insert(p.id.clone(), r.clone());
            }
        }
        for f in &store.status.failed {
            match at.get(&f.goal.id).map(|&i| &mut goals[i]) {
                Some(g) => {
                    if failure_stands(&f.goal, g) {
                        g.state = GoalState::Failed {
                            reason: f.reason.clone(),
                        };
                    } else {
                        reopened.push(f.goal.id.clone());
                    }
                }
                None => {
                    // A ledger goal is the ledger's to derive, failed or parked: a
                    // failure about a row that reads current has nothing left to say.
                    // Mirrors docs/compiler/reconciler.md#parked-and-failed.
                    let ledger_kind = LEDGER_KINDS.contains(&f.goal.kind.as_str());
                    if target_exists(store, &f.goal.target) && !ledger_kind {
                        let mut g = f.goal.clone();
                        g.state = GoalState::Failed {
                            reason: f.reason.clone(),
                        };
                        at.insert(g.id.clone(), goals.len());
                        goals.push(g);
                    } else if ledger_kind {
                        dropped_failed.push(f.goal.id.clone());
                    }
                }
            }
        }
        goals.sort_by(|a, b| a.id.cmp(&b.id));

        // Manual-mode gating first: a gated goal is blocked on a release, not open,
        // so it neither schedules nor holds a cone.
        let mut gated: BTreeSet<String> = BTreeSet::new();
        if control.compile == "manual" {
            for g in goals.iter().filter(|g| is_open(g)) {
                if !matches!(g.kind.as_str(), "reconcile-section" | "place-anchors") {
                    continue;
                }
                let Some(doc) = target_doc(&g.target) else {
                    continue;
                };
                let current = store
                    .docs
                    .get(&doc)
                    .map(|r| r.content_hash.as_str())
                    .unwrap_or_default();
                if control.released.compile.get(&doc).map(String::as_str) != Some(current) {
                    gated.insert(g.id.clone());
                }
            }
        }
        if control.generate == "manual" && control.released.generate != store.status.generation {
            for g in goals.iter().filter(|g| is_open(g)) {
                if matches!(g.kind.as_str(), "bind" | "generate") {
                    gated.insert(g.id.clone());
                }
            }
        }
        for g in goals.iter_mut().filter(|g| gated.contains(&g.id)) {
            let stage = if matches!(g.kind.as_str(), "bind" | "generate") {
                "generate"
            } else {
                "compile"
            };
            g.state = GoalState::Blocked {
                on: format!("release: {}", stage),
            };
        }

        // Records the goals stand on, and the ones whose evidence lapsed. The records
        // are indexed by subject once; a goal's list keeps the records' own order.
        let mut by_subject: BTreeMap<&str, Vec<(usize, &ChangeRecord)>> = BTreeMap::new();
        for (i, c) in store.status.changes.iter().enumerate() {
            by_subject
                .entry(c.subject.as_str())
                .or_default()
                .push((i, c));
        }
        let mut records: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for g in &goals {
            let kinds = goals::record_kinds(&g.kind);
            let subjects: Vec<&str> = match goals::pair_members(&g.target) {
                Some((a, b)) => vec![a, b],
                None => vec![g.target.as_str()],
            };
            let mut hits: Vec<(usize, &str)> = subjects
                .iter()
                .flat_map(|s| by_subject.get(s).into_iter().flatten())
                .filter(|(_, c)| kinds.contains(&c.kind.as_str()))
                .map(|(i, c)| (*i, c.id.as_str()))
                .collect();
            hits.sort_unstable();
            hits.dedup();
            records.insert(
                g.id.clone(),
                hits.into_iter().map(|(_, id)| id.to_string()).collect(),
            );
        }
        let standing: BTreeSet<&String> = records.values().flatten().collect();
        let lapsed: Vec<String> = store
            .status
            .changes
            .iter()
            .filter(|c| {
                matches!(
                    c.kind.as_str(),
                    crate::store::CHANGE_SECTION_DIRTY | crate::store::CHANGE_ANCHOR_STALE
                ) && !standing.contains(&c.id)
            })
            .map(|c| c.id.clone())
            .collect();

        // A selected build counts only its own goals toward readiness.
        let outside: BTreeSet<String> = match crate::selection::current() {
            Some(items) => {
                let sel = crate::selection::Resolved::new(store, &items);
                goals
                    .iter()
                    .filter(|g| !sel.holds(g))
                    .map(|g| g.id.clone())
                    .collect()
            }
            None => BTreeSet::new(),
        };

        // Readiness inputs.
        let mut tier_open = [0usize; 4];
        let mut tier1_open_docs = BTreeSet::new();
        let mut bind_open_by_entity: BTreeMap<String, usize> = BTreeMap::new();
        // A document whose place-anchors goal failed stays undecided: its sections
        // wait behind the failure, and an undecided document blocks its own sections
        // and nothing else, so they hold no later tier or link level.
        // Mirrors docs/compiler/goals/place-anchors.md#readiness.
        let undecided_docs: BTreeSet<&str> = goals
            .iter()
            .filter(|g| g.kind == "place-anchors" && matches!(g.state, GoalState::Failed { .. }))
            .map(|g| g.target.as_str())
            .collect();
        for g in goals
            .iter()
            .filter(|g| is_open(g) && !outside.contains(&g.id))
        {
            if g.kind == "reconcile-section"
                && target_doc(&g.target).is_some_and(|d| undecided_docs.contains(d.as_str()))
            {
                continue;
            }
            if let Some(t) = goals::tier(&g.kind) {
                tier_open[t as usize] += 1;
                if t == 1 {
                    if let Some(d) = target_doc(&g.target) {
                        tier1_open_docs.insert(d);
                    }
                }
            }
            if g.kind == "bind" {
                if let Some(e) = g.change["entity"].as_str() {
                    *bind_open_by_entity.entry(e.to_string()).or_insert(0) += 1;
                }
            }
        }
        let alignment_pending: BTreeSet<String> = store
            .status
            .changes
            .iter()
            .filter(|c| c.kind == crate::store::CHANGE_ALIGNMENT_PENDING)
            .map(|c| c.subject.clone())
            .collect();
        // The indices of this derivation: the referrer index every cone reads and the
        // level of every view. Mirrors docs/compiler/graph.md#derived-data.
        let refs = Refs::build(store);
        // The open compile goals by what a cone would hold them through: a node,
        // section, or document target directly, a pair target through either member.
        // Each cone then looks its blockers up instead of testing every goal, and
        // the list keeps the goals' order.
        let mut compile_by_key: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (i, o) in goals.iter().enumerate() {
            if o.class != "compile" || !is_open(o) || outside.contains(&o.id) {
                continue;
            }
            match goals::pair_members(&o.target) {
                Some((a, b)) => {
                    compile_by_key.entry(a).or_default().push(i);
                    compile_by_key.entry(b).or_default().push(i);
                }
                None => compile_by_key.entry(o.target.as_str()).or_default().push(i),
            }
        }
        let mut cone_blockers: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for g in goals.iter().filter(|g| g.class == "gc") {
            let cone = cone_with(store, &refs, &g.target);
            let mut hits: Vec<usize> = cone
                .held_keys()
                .flat_map(|k| compile_by_key.get(k).into_iter().flatten().copied())
                .collect();
            hits.sort_unstable();
            hits.dedup();
            let blockers: Vec<String> = hits.into_iter().map(|i| goals[i].id.clone()).collect();
            cone_blockers.insert(g.id.clone(), blockers);
        }
        let mut view_levels: BTreeMap<String, String> = BTreeMap::new();
        if goals.iter().any(|g| g.kind == "split-view") {
            let ix = crate::derive::Indices::build(store);
            let levels = crate::derive::view_levels(store, &ix);
            for g in goals.iter().filter(|g| g.kind == "split-view") {
                if let Some(l) = levels.get(&g.target) {
                    view_levels.insert(g.target.clone(), l.clone());
                }
            }
        }

        let by_id: BTreeMap<String, usize> = goals
            .iter()
            .enumerate()
            .map(|(i, g)| (g.id.clone(), i))
            .collect();
        let open_kinds: BTreeSet<(String, String)> = goals
            .iter()
            .filter(|g| is_open(g))
            .map(|g| (g.kind.clone(), g.target.clone()))
            .collect();
        let mut board = Board {
            generation: store.status.generation,
            goals,
            readiness: BTreeMap::new(),
            gated: BTreeSet::new(),
            claimed: BTreeMap::new(),
            held_batches: BTreeSet::new(),
            own: BTreeSet::new(),
            batches: Vec::new(),
            lapsed,
            reopened,
            dropped_parked,
            dropped_failed,
            parked_reasons,
            verdict: store.status.verdict.clone(),
            open_diags: store.open_diag_counts(),
            dangling_diags: store.has_dangling_diags(),
            alignment_pending,
            tier_open,
            doc_levels: doc_levels(store, proj),
            blob_inputs: store
                .docs
                .iter()
                .filter_map(|(doc, rec)| {
                    rec.sections
                        .values()
                        .find(|s| s.kind == "blob")
                        .map(|s| (doc.clone(), crate::input::referring_documents(s)))
                })
                .collect(),
            tier1_open_docs,
            bind_open_by_entity,
            cone_blockers,
            view_levels,
            records,
            by_id,
            open_kinds,
            waiting: pairs.waiting,
            outside,
        };

        // Readiness per kind, then the control plane: a release gate and a lease both
        // render as blocked with the reason.
        let mut readiness: BTreeMap<String, Ready> = BTreeMap::new();
        for g in &board.goals {
            let r = match g.state {
                GoalState::Failed { ref reason } => Ready::Blocked(format!("failed: {}", reason)),
                GoalState::Blocked { ref on } => Ready::Blocked(format!("blocked on {}", on)),
                _ => goals::kind(&g.kind)
                    .map(|k| k.ready(g, &board))
                    .unwrap_or(Ready::Blocked("unknown kind".into())),
            };
            readiness.insert(g.id.clone(), r);
        }
        board.readiness = readiness;
        for id in &gated {
            let stage = if id.starts_with("g:bind:") || id.starts_with("g:generate:") {
                "generate"
            } else {
                "compile"
            };
            board.readiness.insert(
                id.clone(),
                Ready::Blocked(format!(
                    "awaiting release: `jazyk release {}` (or the GUI) approves it",
                    stage
                )),
            );
        }
        board.gated = gated;
        for id in &board.outside {
            board
                .readiness
                .insert(id.clone(), Ready::Blocked("outside the selection".into()));
        }
        let leases = crate::control::task_leases(&store.out);
        if !leases.is_empty() {
            let mut claimed = BTreeMap::new();
            for g in &board.goals {
                let mut keys: Vec<String> = vec![g.id.clone(), g.target.clone()];
                if let Some((a, b)) = goals::pair_members(&g.target) {
                    keys.push(a.to_string());
                    keys.push(b.to_string());
                }
                if let Some(d) = target_doc(&g.target) {
                    keys.push(d);
                }
                let hit = keys
                    .iter()
                    .find_map(|k| leases.get(k))
                    .or_else(|| leases.values().find(|l| l.goals.iter().any(|x| x == &g.id)));
                if let Some(l) = hit {
                    claimed.insert(g.id.clone(), l.worker.clone());
                }
            }
            for (id, worker) in &claimed {
                board.readiness.insert(
                    id.clone(),
                    Ready::Blocked(format!(
                        "claimed by worker `{}` (the lease lapses {}s after its last heartbeat)",
                        worker,
                        crate::control::LEASE_TTL_SECS
                    )),
                );
            }
            board.claimed = claimed;
            board.held_batches = leases.keys().filter(|k| is_batch_id(k)).cloned().collect();
        }
        board.batches = board.form_batches(store, proj);
        board
    }

    // Load the store, align the documents in memory, read the control plane, derive.
    // A read-only derivation never commits: the `edit` and `align` entries are a
    // build's to write (a stray argument read as a document path once emptied a
    // project's section tree through this call).
    // Mirrors docs/compiler/reconciler.md#goal-derivation.
    pub fn compute(proj: &Project, out: &Path) -> Board {
        // The browser engine's tree changes only through its own writes, and a page
        // opening asks for the board several times over one tree: a board of an
        // unchanged tree and project is the last one's copy. A board holding leases is
        // never reused, since a lease lapses with time, not with a write.
        #[cfg(target_arch = "wasm32")]
        {
            if crate::selection::current().is_some() {
                return Self::compute_uncached(proj, out);
            }
            thread_local! {
                static LAST: std::cell::RefCell<BTreeMap<String, (u64, Board)>> = const { std::cell::RefCell::new(BTreeMap::new()) };
            }
            let key = memo_key(proj, out);
            let now = crate::vfs::memory_generation();
            if let Some(board) = LAST.with(|l| {
                l.borrow()
                    .get(&key)
                    .filter(|(g, _)| *g == now)
                    .map(|(_, b)| b.clone())
            }) {
                return board;
            }
            let board = Self::compute_uncached(proj, out);
            let now = crate::vfs::memory_generation();
            LAST.with(|l| {
                let mut l = l.borrow_mut();
                l.retain(|_, (g, _)| *g == now);
                if board.claimed.is_empty() && board.held_batches.is_empty() {
                    l.insert(key, (now, board.clone()));
                }
            });
            board
        }
        #[cfg(not(target_arch = "wasm32"))]
        Self::compute_uncached(proj, out)
    }

    fn compute_uncached(proj: &Project, out: &Path) -> Board {
        let mut store = Store::load(out);
        let (parsed, _) = crate::reconcile::parse_all(proj);
        store.sync_docs_in_memory(&parsed);
        let control = Control::load(proj, out);
        Board::derive(&store, proj, &control)
    }
}
