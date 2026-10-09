use crate::vfs::VPathExt;
// The live event stream. One SSE feed multiplexes job lifecycle, store changes,
// document changes, and worklist sizes. Mirrors docs/frontends/gui.md#events.
//
// Every event carries a monotonic `seq`. A ring of recent events serves
// Last-Event-ID replay on reconnect; a gap beyond the ring yields `resync` and the
// client refetches its snapshots.
use super::state::SharedState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use futures_util::stream::{self, Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::convert::Infallible;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

const RING: usize = 512;

#[derive(Clone)]
pub struct EventHub {
    inner: Arc<EventInner>,
    set_id: Option<String>,
}
struct EventInner {
    tx: broadcast::Sender<Arc<Value>>,
    seq: AtomicU64,
    ring: Mutex<VecDeque<Arc<Value>>>,
}

impl EventHub {
    pub fn new() -> EventHub {
        let (tx, _) = broadcast::channel(256);
        EventHub {
            inner: Arc::new(EventInner {
                tx,
                seq: AtomicU64::new(0),
                ring: Mutex::new(VecDeque::new()),
            }),
            set_id: None,
        }
    }
    pub fn scoped(&self, set_id: Option<String>) -> Self {
        Self {
            inner: self.inner.clone(),
            set_id,
        }
    }

    // Callable from any thread: broadcast send and the ring are both sync.
    pub fn emit(&self, kind: &str, mut payload: Value) {
        let seq = self.inner.seq.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(id) = &self.set_id {
            payload = crate::webapi::sets::qualify_value(payload, id);
            payload["setId"] = json!(id);
        }
        payload["type"] = json!(kind);
        payload["seq"] = json!(seq);
        let ev = Arc::new(payload);
        {
            let mut ring = self.inner.ring.lock().unwrap();
            ring.push_back(ev.clone());
            while ring.len() > RING {
                ring.pop_front();
            }
        }
        let _ = self.inner.tx.send(ev);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Value>> {
        self.inner.tx.subscribe()
    }

    // Events after `last`, or None when the gap exceeds the ring (client must resync).
    fn replay_after(&self, last: u64) -> Option<Vec<Arc<Value>>> {
        let ring = self.inner.ring.lock().unwrap();
        let oldest = ring
            .front()
            .map(|e| e["seq"].as_u64().unwrap_or(0))
            .unwrap_or(0);
        let newest = ring
            .back()
            .map(|e| e["seq"].as_u64().unwrap_or(0))
            .unwrap_or(0);
        if last >= newest {
            return Some(Vec::new());
        }
        if last + 1 < oldest {
            return None;
        }
        Some(
            ring.iter()
                .filter(|e| e["seq"].as_u64().unwrap_or(0) > last)
                .cloned()
                .collect(),
        )
    }
}

fn to_sse(ev: &Value) -> SseEvent {
    let id = ev["seq"].as_u64().unwrap_or(0);
    SseEvent::default().id(id.to_string()).data(ev.to_string())
}

fn resync_event() -> SseEvent {
    SseEvent::default().data(json!({ "type": "resync" }).to_string())
}

pub async fn sse(
    State(st): State<SharedState>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    // Subscribe before reading the ring so nothing falls between replay and live.
    let rx = st.events.subscribe();
    let last_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let mut opening: Vec<SseEvent> = Vec::new();
    if let Some(last) = last_id {
        match st.events.replay_after(last) {
            Some(missed) => opening.extend(missed.iter().map(|e| to_sse(e))),
            None => opening.push(resync_event()),
        }
    }
    let live = tokio_stream::wrappers::BroadcastStream::new(rx).map(|item| match item {
        Ok(ev) => to_sse(&ev),
        // The consumer lagged past the channel capacity: tell it to refetch.
        Err(_) => resync_event(),
    });
    let s = stream::iter(opening).chain(live).map(Ok);
    Sse::new(s).keep_alive(KeepAlive::default())
}

// Cheap generation read: one line of status.yaml, no shard parsing.
pub fn read_generation(out: &Path) -> u64 {
    crate::store::read_generation(out)
}

// Condense a journal entry for the event stream: the entry kind, the goal batch, and one
// {op, id} per mutation. Full bodies stay behind /api/journal.
fn entry_summary(g: u64, entry: &Value) -> Value {
    let ops: Vec<Value> = entry["mutations"]
        .as_array()
        .map(|ms| {
            ms.iter()
                .map(|m| json!({ "op": m["op"], "id": m["id"] }))
                .collect()
        })
        .unwrap_or_default();
    json!({
        "generation": g,
        "build": entry["build"],
        "kind": entry["kind"],
        "batch": entry["batch"],
        "resolvedGoals": entry["resolved_goals"],
        "openedGoals": entry["opened_goals"],
        "ops": ops,
        "rounds": entry["rounds"],
        "tokens": entry["tokens"],
    })
}

// Causes landing within this window share one derivation (gui.md#events).
const BOARD_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(300);

// One board derivation per document set at a time. A cause marks the set dirty; a
// worker thread per dirty set sleeps the window, derives once, and goes on while
// more causes landed. Shared by every scoped state of the process.
#[derive(Default)]
pub struct BoardCoalescer {
    sets: Mutex<BTreeMap<String, BoardSlot>>,
}

#[derive(Default)]
struct BoardSlot {
    dirty: bool,
    running: bool,
    // The scoped state of the latest cause: what the derivation runs against.
    latest: Option<SharedState>,
}

// Re-derive the board off the calling thread and emit board.changed with the
// counts; the client refetches GET /api/board for the cards. Bursts coalesce per
// set behind BOARD_DEBOUNCE. Mirrors docs/frontends/gui.md#events.
pub fn emit_board_changed(st: &SharedState) {
    let key = st.proj().set_id.unwrap_or_default();
    let coalescer = st.board.clone();
    {
        let mut sets = coalescer.sets.lock().unwrap();
        let slot = sets.entry(key.clone()).or_default();
        slot.dirty = true;
        slot.latest = Some(st.clone());
        if slot.running {
            return;
        }
        slot.running = true;
    }
    std::thread::spawn(move || loop {
        std::thread::sleep(BOARD_DEBOUNCE);
        let scope = {
            let mut sets = coalescer.sets.lock().unwrap();
            let slot = sets.entry(key.clone()).or_default();
            if !slot.dirty {
                slot.running = false;
                slot.latest = None;
                return;
            }
            slot.dirty = false;
            match slot.latest.clone() {
                Some(s) => s,
                None => {
                    slot.running = false;
                    return;
                }
            }
        };
        let board = crate::board::Board::compute(&scope.proj(), &scope.out);
        scope.events.emit(
            "board.changed",
            json!({
                "counts": board.counts(),
                "verdict": board.verdict().to_string(),
            }),
        );
    });
}

// One event per resolved and opened goal of a committed changeset, from the
// journal entries at commit, so a build run anywhere moves the board.
fn emit_goal_events(st: &SharedState, entries: &[Value]) {
    for entry in entries {
        let generation = entry["generation"].clone();
        let batch = entry["batch"].clone();
        for r in entry["resolvedGoals"].as_array().into_iter().flatten() {
            st.events.emit(
                "goal.resolved",
                json!({
                    "goal": r["goal"],
                    "justification": r["justification"],
                    "generation": generation,
                    "batch": batch,
                }),
            );
        }
        for o in entry["openedGoals"].as_array().into_iter().flatten() {
            st.events.emit(
                "goal.opened",
                json!({
                    "goal": o["goal"],
                    "cause": o["cause"],
                    "generation": generation,
                    "batch": batch,
                }),
            );
        }
    }
}

// Recompute the worklist sizes and emit pending.changed when they moved.
pub fn recompute_pending(st: &SharedState) {
    let store = crate::store::Store::load(&st.out);
    let gen_n = crate::gen::pending(&store, &st.gs()).len();
    let verify = crate::verify::pending_counts(&store, &st.gs());
    let now = json!({ "gen": gen_n, "verify": verify });
    let mut last = st.last_pending.lock().unwrap();
    if *last != now {
        *last = now.clone();
        st.events.emit("pending.changed", now);
    }
}

// Watch the store from outside any build: the lock marks builds starting and ending
// (this process or any other), and each generation bump carries its journal entries.
// The LSP watcher is the reference; this one emits events instead of stderr lines.
// A plain thread: every step is a disk read, which has no place on the runtime.
pub fn spawn_store_watcher(st: SharedState) {
    std::thread::spawn(move || {
        let mut previous = std::collections::BTreeMap::new();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let Ok(w) = crate::document_sets::Workspace::load(&st.root) else {
                continue;
            };
            previous.retain(|id, _| w.sets.iter().any(|s| &s.id == id));
            for set in &w.sets {
                let Ok(scope) = st.scoped_in(&w, Some(&set.id)) else {
                    continue;
                };
                let now = read_generation(&scope.out);
                let locked = scope.out.join(".lock").vexists();
                let (last, lock_seen) = previous
                    .insert(set.id.clone(), (now, locked))
                    .unwrap_or((now, locked));
                if locked != lock_seen {
                    scope.events.emit("store.lock", json!({"held":locked}));
                }
                if now == last {
                    continue;
                }
                let mut entries = Vec::new();
                for g in (last + 1)..=now {
                    if let Ok(text) = crate::vfs::read_to_string(
                        scope.out.join("journal").join(format!("g{}.yaml", g)),
                    ) {
                        if let Ok(entry) = serde_norway::from_str::<Value>(&text) {
                            entries.push(entry_summary(g, &entry));
                        }
                    }
                }
                scope.events.emit(
                    "store.generation",
                    json!({"generation":now,"entries":entries}),
                );
                emit_goal_events(&scope, &entries);
                emit_board_changed(&scope);
                recompute_pending(&scope);
            }
        }
    });
}

// Emits control.changed when a set's control plane moves: a release, a mode change,
// a worker registering or dropping, a lease taken or freed. Polling: the surfaces
// are tiny files, and a 2 second cadence is livelier than a heartbeat needs. The
// event is scoped to the set whose files moved, and its board re-derives behind it.
// Mirrors docs/frontends/gui.md#events.
pub fn spawn_control_watcher(st: SharedState) {
    std::thread::spawn(move || {
        let fingerprint = |out: &Path| -> String {
            let mut s = String::new();
            let meta = |p: &std::path::Path| {
                std::fs::metadata(p)
                    .map(|m| format!("{}:{:?};", m.len(), m.modified().ok()))
                    .unwrap_or_default()
            };
            s.push_str(&meta(&crate::control::Control::path(out)));
            for dir in ["workers", "leases"] {
                if let Ok(entries) = crate::vfs::read_dir(out.join(dir)) {
                    let mut names: Vec<String> = entries
                        .flatten()
                        .map(|e| format!("{}{}", e.file_name().to_string_lossy(), meta(&e.path())))
                        .collect();
                    names.sort();
                    s.push_str(&names.join(""));
                }
            }
            s
        };
        // Per set: the fingerprint of its control files.
        let all = || -> BTreeMap<String, (std::path::PathBuf, String)> {
            let Ok(w) = crate::document_sets::Workspace::load(&st.root) else {
                return BTreeMap::new();
            };
            w.sets
                .iter()
                .filter_map(|s| w.resolve(Some(&s.id)).ok())
                .map(|p| {
                    let fp = fingerprint(&p.out);
                    (p.set_id.clone().unwrap_or_default(), (p.out, fp))
                })
                .collect()
        };
        let mut last = all();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let now = all();
            let moved: Vec<String> = now
                .iter()
                .filter(|(id, (_, fp))| last.get(*id).map(|(_, l)| l) != Some(fp))
                .map(|(id, _)| id.clone())
                .collect();
            last = now;
            if moved.is_empty() {
                continue;
            }
            let Ok(w) = crate::document_sets::Workspace::load(&st.root) else {
                continue;
            };
            for id in moved {
                let Ok(scope) = st.scoped_in(&w, Some(&id)) else {
                    continue;
                };
                let snap = super::api::workers_snapshot(&scope);
                scope.events.emit("control.changed", snap);
                emit_board_changed(&scope);
            }
        }
    });
}

// Emits docs.changed; the watch-mode compile trigger hooks in here.
pub fn spawn_docs_watcher(st: SharedState) {
    std::thread::spawn(move || {
        use notify::Watcher;
        let fingerprints = || -> std::collections::BTreeMap<String, String> {
            let Ok(w) = crate::document_sets::Workspace::load(&st.root) else {
                return Default::default();
            };
            let mut values = std::collections::BTreeMap::from([(
                crate::document_sets::CONFIG.into(),
                w.hash.clone(),
            )]);
            for f in w
                .files()
                .into_iter()
                .filter(|f| !w.memberships(f).is_empty())
            {
                let fp = std::fs::metadata(st.root.join(&f))
                    .map(|m| format!("{}:{:?}", m.len(), m.modified().ok()))
                    .unwrap_or_default();
                values.insert(f, fp);
            }
            values
        };
        let mut last = fingerprints();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match notify::recommended_watcher(move |res: Result<notify::Event, _>| {
            if res.is_ok() {
                let _ = tx.send(());
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                eprintln!(
                    "[jazyk] warning: no file watcher for {}: {}; document changes will not be noticed until a restart",
                    st.root.display(),
                    e
                );
                return;
            }
        };
        if let Err(e) = watcher.watch(&st.root, notify::RecursiveMode::Recursive) {
            eprintln!(
                "[jazyk] warning: cannot watch {}: {}; document changes will not be noticed until a restart",
                st.root.display(),
                e
            );
            return;
        }
        while rx.recv().is_ok() {
            std::thread::sleep(std::time::Duration::from_millis(300));
            while rx.try_recv().is_ok() {}
            let now = fingerprints();
            if now == last {
                continue;
            }
            let changed: std::collections::BTreeSet<_> = now
                .keys()
                .chain(last.keys())
                .filter(|k| now.get(*k) != last.get(*k))
                .cloned()
                .collect();
            last = now;
            let config = changed.contains(crate::document_sets::CONFIG);
            if config {
                st.events.emit("sets.changed", json!({}));
                super::reload_project(&st);
            }
            let Ok(w) = crate::document_sets::Workspace::load(&st.root) else {
                continue;
            };
            for set in w.sets {
                if config
                    || changed.iter().any(|f| {
                        crate::document_sets::included(&set.docs, f)
                            || crate::document_sets::included(&set.source, f)
                    })
                {
                    let Ok(scope) = st.scoped(Some(&set.id)) else {
                        continue;
                    };
                    scope
                        .events
                        .emit("docs.changed", json!({"docs":changed,"graphStale":true}));
                    emit_board_changed(&scope);
                    super::jobs_hook_on_docs_changed(&scope);
                }
            }
        }
    });
}
