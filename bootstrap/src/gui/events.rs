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
    // This process's start, part of every event id: an id from an earlier server
    // (the client outlived a restart) never resumes against this ring.
    boot: u64,
    seq: AtomicU64,
    ring: Mutex<VecDeque<Arc<Value>>>,
}

impl EventHub {
    pub fn new() -> EventHub {
        let (tx, _) = broadcast::channel(256);
        EventHub {
            inner: Arc::new(EventInner {
                tx,
                boot: crate::sys::now_millis() as u64,
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

    // The SSE id of an event: `<boot>-<seq>`.
    fn event_id(&self, ev: &Value) -> String {
        format!("{}-{}", self.inner.boot, ev["seq"].as_u64().unwrap_or(0))
    }

    // Events after a client's Last-Event-ID, or None when the server cannot account
    // for the gap: another process issued the id, the id is malformed or ahead of
    // this server, or the gap exceeds the ring. The client must resync.
    fn replay_after_id(&self, id: &str) -> Option<Vec<Arc<Value>>> {
        let (boot, seq) = id.split_once('-')?;
        if boot.parse::<u64>().ok()? != self.inner.boot {
            return None;
        }
        let last = seq.parse::<u64>().ok()?;
        if last > self.inner.seq.load(Ordering::SeqCst) {
            return None;
        }
        self.replay_after(last)
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

fn to_sse(hub: &EventHub, ev: &Value) -> SseEvent {
    SseEvent::default()
        .id(hub.event_id(ev))
        .data(ev.to_string())
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
        .map(|v| v.to_str().unwrap_or_default().to_string());
    let hub = st.events.clone();
    let mut opening: Vec<SseEvent> = Vec::new();
    if let Some(last) = last_id {
        match hub.replay_after_id(&last) {
            Some(missed) => opening.extend(missed.iter().map(|e| to_sse(&hub, e))),
            None => opening.push(resync_event()),
        }
    }
    let live = tokio_stream::wrappers::BroadcastStream::new(rx).map(move |item| match item {
        Ok(ev) => to_sse(&hub, &ev),
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
    if last.get(&st.set_key()) != Some(&now) {
        last.insert(st.set_key(), now.clone());
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
                // A set's out directory is fixed by its id; its scoped state (project,
                // settings, model configuration) is resolved only when it moved.
                let out = w.root.join(".jazyk").join(&set.id);
                let now = read_generation(&out);
                let locked = out.join(".lock").vexists();
                if previous
                    .get(&set.id)
                    .is_none_or(|seen| *seen == (now, locked))
                {
                    previous.insert(set.id.clone(), (now, locked));
                    continue;
                }
                let Ok(scope) = st.scoped_in(&w, Some(&set.id)) else {
                    continue;
                };
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
        let (tx, rx) = std::sync::mpsc::channel::<Vec<std::path::PathBuf>>();
        let mut watcher = match notify::recommended_watcher(move |res: Result<notify::Event, _>| {
            if let Ok(event) = res {
                let _ = tx.send(event.paths);
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
        let mut watched = std::collections::BTreeSet::new();
        if let Err(e) = watch_tree(&mut watcher, &st.root, &mut watched) {
            eprintln!(
                "[jazyk] warning: cannot watch {}: {}; document changes will not be noticed until a restart",
                st.root.display(),
                e
            );
            return;
        }
        let relevant =
            |paths: &[std::path::PathBuf]| paths.iter().any(|p| watched_path(&st.root, p));
        while let Ok(paths) = rx.recv() {
            // Build caches and dot-folders never hold a document: their churn (a
            // `cargo build`, an `npm install`) costs no fingerprint pass.
            let mut hit = relevant(&paths);
            std::thread::sleep(std::time::Duration::from_millis(300));
            while let Ok(more) = rx.try_recv() {
                hit |= relevant(&more);
            }
            // A folder new at the top level gets its own watch.
            let _ = watch_tree(&mut watcher, &st.root, &mut watched);
            if !hit {
                continue;
            }
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

// Whether a changed path can matter to a document set: the configuration, or a
// file a set may claim (no dot-folder, no `target/`, no `node_modules/`).
fn watched_path(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    let rel = rel.to_string_lossy().replace('\\', "/");
    rel == crate::document_sets::CONFIG
        || rel == ".jazyk"
        || rel.is_empty()
        || crate::document_sets::eligible(&rel)
}

// Watch the root itself and `.jazyk/` without descending, and each top-level folder
// a set may claim recursively, so build caches and dot-folders at the top cost no
// watch. Already watched folders are skipped; a new top-level folder is added on
// the next call.
fn watch_tree(
    watcher: &mut impl notify::Watcher,
    root: &Path,
    watched: &mut std::collections::BTreeSet<std::path::PathBuf>,
) -> notify::Result<()> {
    use notify::RecursiveMode;
    if watched.insert(root.to_path_buf()) {
        watcher.watch(root, RecursiveMode::NonRecursive)?;
    }
    let meta = root.join(".jazyk");
    if meta.is_dir() && watched.insert(meta.clone()) {
        let _ = watcher.watch(&meta, RecursiveMode::NonRecursive);
    }
    for dir in top_level_watch_dirs(root) {
        if watched.insert(dir.clone()) {
            let _ = watcher.watch(&dir, RecursiveMode::Recursive);
        }
    }
    Ok(())
}

fn top_level_watch_dirs(root: &Path) -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<_> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| crate::document_sets::eligible(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    // The docs watcher skips what no set can claim: dot-folders, `target/`, and
    // `node_modules/` cost neither a watch at the top nor a fingerprint pass.
    #[test]
    fn the_docs_watcher_skips_build_caches_and_dot_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for d in [
            "docs",
            "src",
            "target",
            "node_modules",
            ".git",
            ".github",
            ".jazyk",
        ] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let names: Vec<String> = top_level_watch_dirs(root)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![".github", "docs", "src"]);
        assert!(watched_path(root, &root.join("docs/a.md")));
        assert!(watched_path(root, &root.join(crate::document_sets::CONFIG)));
        assert!(!watched_path(root, &root.join("target/debug/x.o")));
        assert!(!watched_path(
            root,
            &root.join("bootstrap/target/debug/x.o")
        ));
        assert!(!watched_path(root, &root.join("web/node_modules/a/b.js")));
        assert!(!watched_path(root, &root.join(".git/index")));
        assert!(!watched_path(
            root,
            &root.join(".jazyk/ds_main/graph/x.yaml")
        ));
    }

    // A Last-Event-ID resumes only against the process that issued it: an id from a
    // restarted server, a malformed id, or one ahead of this server resyncs
    // (docs/frontends/gui.md#events).
    #[test]
    fn replay_resumes_only_ids_this_server_issued() {
        let hub = EventHub::new();
        for _ in 0..3 {
            hub.emit("x", json!({}));
        }
        let ring: Vec<Value> = hub
            .inner
            .ring
            .lock()
            .unwrap()
            .iter()
            .map(|e| (**e).clone())
            .collect();
        let first = hub.event_id(&ring[0]);
        let missed = hub.replay_after_id(&first).unwrap();
        assert_eq!(missed.len(), 2);
        assert!(hub
            .replay_after_id(&hub.event_id(&ring[2]))
            .unwrap()
            .is_empty());
        let boot = hub.inner.boot;
        assert!(hub.replay_after_id(&format!("{}-1", boot + 1)).is_none());
        assert!(hub.replay_after_id(&format!("{}-99", boot)).is_none());
        assert!(hub.replay_after_id("2").is_none());
        assert!(hub.replay_after_id("").is_none());
    }
}

#[cfg(test)]
mod replay_tests;
