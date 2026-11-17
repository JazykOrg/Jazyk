// The session loop with several sessions in flight: each session runs on its own
// thread against a runner the threads share, and commits through its own serving as
// a sequential session does. The loop keeps the board, the store, and the tallies on
// the build's thread: it opens batches while slots are free, waits for any session to
// end, settles it, re-derives the board, and fills the freed slots. Native only; a
// browser tab and the hosted runtime run the sequential loop.
// Mirrors docs/compiler/reconciler.md#scheduling.
use super::*;
use crate::board::{Batch, Cone, Refs};
use crate::runner::SessionRunner;

// One session in flight: the batch as the loop opened it, the executor slot it takes,
// and the cone its goal targets cover, for the overlap test.
struct InFlight {
    active: ActiveBatch,
    batch: Batch,
    slot: String,
    targets: Vec<String>,
    cone: Cone,
    line: InFlightBatch,
}

// The goal targets of a batch, as the board holds them.
fn targets(board: &Board, batch: &Batch) -> Vec<String> {
    batch
        .goals
        .iter()
        .filter_map(|id| board.goal(id))
        .map(|g| g.target.clone())
        .collect()
}

// The union of the targets' cones (docs/compiler/reconciler.md#cones).
fn cone_of(store: &Store, refs: &Refs, targets: &[String]) -> Cone {
    let mut cone = Cone {
        nodes: BTreeSet::new(),
        sections: BTreeSet::new(),
        docs: BTreeSet::new(),
    };
    for t in targets {
        let c = crate::board::cone_with(store, refs, t);
        cone.nodes.extend(c.nodes);
        cone.sections.extend(c.sections);
        cone.docs.extend(c.docs);
    }
    cone
}

// Whether a candidate overlaps a session in flight: one locality key, or a target of
// either inside the other's cone.
fn overlaps(store: &Store, refs: &Refs, board: &Board, batch: &Batch, flight: &[InFlight]) -> bool {
    let mine = targets(board, batch);
    let mut cone: Option<Cone> = None;
    flight.iter().any(|f| {
        if !batch.locality.is_empty() && batch.locality == f.batch.locality {
            return true;
        }
        if mine.iter().any(|t| f.cone.holds_target(t)) {
            return true;
        }
        let c = cone.get_or_insert_with(|| cone_of(store, refs, &mine));
        f.targets.iter().any(|t| c.holds_target(t))
    })
}

// The next batch to open, or none while every candidate waits: the tier barrier, the
// executor's free slots, and no overlap with this build's sessions in flight. A batch
// another worker has open runs only when nothing else may.
fn pick(
    runner: &dyn SessionRunner,
    store: &Store,
    board: &Board,
    candidates: &[Batch],
    flight: &[InFlight],
) -> Option<(Batch, String)> {
    let lowest_tier = flight
        .iter()
        .filter(|f| f.batch.class == goals::Class::Compile)
        .filter_map(|f| f.batch.tier)
        .min();
    let refs = (!flight.is_empty()).then(|| Refs::build(store));
    let mut fallback = None;
    for b in candidates {
        if b.class == goals::Class::Compile {
            if let (Some(low), Some(tier)) = (lowest_tier, b.tier) {
                if tier > low {
                    continue;
                }
            }
        }
        let (slot, n) = runner.session_slots(&batch_run(board, b));
        if flight.iter().filter(|f| f.slot == slot).count() >= n.max(1) {
            continue;
        }
        if let Some(refs) = &refs {
            if overlaps(store, refs, board, b, flight) {
                continue;
            }
        }
        if b.goals.iter().any(|g| board.open_by.contains_key(g)) {
            if fallback.is_none() {
                fallback = Some((b.clone(), slot));
            }
            continue;
        }
        return Some((b.clone(), slot));
    }
    fallback
}

// Run the build's sessions concurrently until no batch is runnable or a budget is
// spent. Returns the reason the build suspends, when it does.
pub(super) fn run(
    lp: &Loop,
    runner: &(dyn SessionRunner + Sync),
    store: &mut Store,
    board: &mut Board,
    t: &mut Tally,
    record: &mut Option<&mut crate::control::RunRecord>,
    worker: &str,
) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel::<(String, std::thread::Result<SessionReport>)>();
    std::thread::scope(|scope| {
        let mut flight: Vec<InFlight> = Vec::new();
        // No session opens once the build is cancelled, suspended, interrupted, or out
        // of budget; the sessions in flight still finish and settle.
        let mut halted = false;
        let mut suspend: Option<String> = None;
        let lines = |flight: &[InFlight]| flight.iter().map(|f| f.line.clone()).collect();
        loop {
            if !halted {
                lp.widen(store, board);
            }
            while !halted {
                if lp.trace.is_cancelled() {
                    halted = true;
                    break;
                }
                if crate::session_recovery::suspension_requested() {
                    suspend = Some("suspended".into());
                    halted = true;
                    break;
                }
                t.derived_ids
                    .extend(board.selected_open_goals().iter().map(|g| g.id.clone()));
                // Goals parked this build take no batch, and neither do the goals of
                // its sessions in flight; their neighbors still run.
                board.withhold(&t.parked_ids, store, lp.proj);
                let held: BTreeMap<String, Vec<String>> = flight
                    .iter()
                    .map(|f| (f.batch.id.clone(), f.batch.goals.clone()))
                    .collect();
                board.hold_in_flight(&held, store, lp.proj);
                let candidates = lp.candidates(board, t);
                if candidates.is_empty() {
                    break;
                }
                if lp.exhausted(t) {
                    // The sessions in flight finish first; then the open goals park.
                    if flight.is_empty() {
                        lp.exhaust(t, board);
                        halted = true;
                    }
                    break;
                }
                let Some((batch, slot)) = pick(runner, store, board, &candidates, &flight) else {
                    break;
                };
                let active = lp.open(board, &batch);
                t.sessions += 1;
                // The session's scoped serving opens exactly this batch under this id,
                // however a later board numbers it (docs/frontends/mcp.md#mcp-into-acp-sessions).
                crate::control::claim_goals(lp.out, &batch.id, worker, &batch.goals).ok();
                let targets = targets(board, &batch);
                let cone = cone_of(store, &Refs::build(store), &targets);
                let line = in_flight_line(&active);
                let run = active.run.clone();
                let (tx, trace) = (tx.clone(), lp.trace.clone());
                scope.spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        runner.run_item(&run, &trace)
                    }));
                    tx.send((run.id.clone(), result)).ok();
                });
                flight.push(InFlight {
                    active,
                    batch,
                    slot,
                    targets,
                    cone,
                    line,
                });
                t.record(record, lines(&flight));
            }
            if flight.is_empty() {
                if halted {
                    break;
                }
                // A selection widened during the last batch runs before the build
                // ends; otherwise the selection closes and later requests queue.
                match crate::selection::handle() {
                    Some(h) if !h.seal_unless_widened() => continue,
                    _ => break,
                }
            }
            // The entries of the sessions in flight stay fresh while they run.
            let (id, result) = loop {
                match rx.recv_timeout(std::time::Duration::from_secs(30)) {
                    Ok(message) => break message,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        for f in &flight {
                            crate::control::refresh_lease(lp.out, &f.batch.id, worker);
                        }
                    }
                    Err(e) => panic!("a session in flight stopped reporting: {e}"),
                }
            };
            crate::control::release_lease(lp.out, &id, worker);
            let at = flight
                .iter()
                .position(|f| f.active.run.id == id)
                .expect("the reporting session is in flight");
            let done = flight.remove(at);
            let report = match result {
                Ok(report) => report,
                Err(panic) => std::panic::resume_unwind(panic),
            };
            if let Some(reason) = interruption(&report) {
                suspend.get_or_insert(reason);
                halted = true;
                t.record(record, lines(&flight));
                continue;
            }
            lp.settle(t, store, board, &done.active, &report, true);
            crate::session_recovery::remove_frame(&crate::session_recovery::batch_key(lp.out, &id));
            t.record(record, lines(&flight));
        }
        suspend
    })
}
