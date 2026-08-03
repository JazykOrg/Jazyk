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
use std::collections::VecDeque;
use std::convert::Infallible;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

const RING: usize = 512;

pub struct EventHub {
    tx: broadcast::Sender<Arc<Value>>,
    seq: AtomicU64,
    ring: Mutex<VecDeque<Arc<Value>>>,
}

impl EventHub {
    pub fn new() -> EventHub {
        let (tx, _) = broadcast::channel(256);
        EventHub { tx, seq: AtomicU64::new(0), ring: Mutex::new(VecDeque::new()) }
    }

    // Callable from any thread: broadcast send and the ring are both sync.
    pub fn emit(&self, kind: &str, mut payload: Value) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        payload["type"] = json!(kind);
        payload["seq"] = json!(seq);
        let ev = Arc::new(payload);
        {
            let mut ring = self.ring.lock().unwrap();
            ring.push_back(ev.clone());
            while ring.len() > RING {
                ring.pop_front();
            }
        }
        let _ = self.tx.send(ev);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Value>> {
        self.tx.subscribe()
    }

    // Events after `last`, or None when the gap exceeds the ring (client must resync).
    fn replay_after(&self, last: u64) -> Option<Vec<Arc<Value>>> {
        let ring = self.ring.lock().unwrap();
        let oldest = ring.front().map(|e| e["seq"].as_u64().unwrap_or(0)).unwrap_or(0);
        let newest = ring.back().map(|e| e["seq"].as_u64().unwrap_or(0)).unwrap_or(0);
        if last >= newest {
            return Some(Vec::new());
        }
        if last + 1 < oldest {
            return None;
        }
        Some(ring.iter().filter(|e| e["seq"].as_u64().unwrap_or(0) > last).cloned().collect())
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

// Condense a journal entry for the event stream: the work item and one {op, id} per
// mutation. Full bodies stay behind /api/journal.
fn entry_summary(g: u64, entry: &Value) -> Value {
    let ops: Vec<Value> = entry["mutations"]
        .as_array()
        .map(|ms| ms.iter().map(|m| json!({ "op": m["op"], "id": m["id"] })).collect())
        .unwrap_or_default();
    json!({
        "generation": g,
        "build": entry["build"],
        "workItem": entry["workItem"],
        "ops": ops,
        "rounds": entry["rounds"],
        "tokens": entry["tokens"],
    })
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
pub fn spawn_store_watcher(st: SharedState) {
    tokio::spawn(async move {
        let mut last_gen = read_generation(&st.out);
        let mut lock_seen = st.out.join(".lock").exists();
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let lock_now = st.out.join(".lock").exists();
            if lock_now != lock_seen {
                lock_seen = lock_now;
                st.events.emit("store.lock", json!({ "held": lock_now }));
            }
            let gen_now = read_generation(&st.out);
            if gen_now == last_gen {
                continue;
            }
            let mut entries: Vec<Value> = Vec::new();
            if gen_now > last_gen {
                for g in (last_gen + 1)..=gen_now {
                    let path = st.out.join("journal").join(format!("g{}.yaml", g));
                    let Ok(text) = std::fs::read_to_string(&path) else { continue };
                    let Ok(entry) = serde_norway::from_str::<Value>(&text) else { continue };
                    entries.push(entry_summary(g, &entry));
                }
            }
            last_gen = gen_now;
            st.events
                .emit("store.generation", json!({ "generation": gen_now, "entries": entries }));
            let st2 = st.clone();
            tokio::task::spawn_blocking(move || recompute_pending(&st2));
        }
    });
}

// Watch the documents with native file events: debounce bursts, then diff a
// per-file fingerprint so editor temp files and out-dir writes never signal.
// Emits control.changed when the control plane moves: a release, a mode change, a
// worker registering or dropping, a lease taken or freed. Polling: the surfaces are
// tiny files, and a 2 second cadence is livelier than a heartbeat needs.
// Mirrors docs/frontends/gui.md#events.
pub fn spawn_control_watcher(st: SharedState) {
    std::thread::spawn(move || {
        let fingerprint = |st: &SharedState| -> String {
            let mut s = String::new();
            let meta = |p: &std::path::Path| {
                std::fs::metadata(p).map(|m| format!("{}:{:?};", m.len(), m.modified().ok())).unwrap_or_default()
            };
            s.push_str(&meta(&crate::control::Control::path(&st.out)));
            for dir in ["workers", "leases"] {
                if let Ok(entries) = std::fs::read_dir(st.out.join(dir)) {
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
        let mut last = fingerprint(&st);
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let now = fingerprint(&st);
            if now != last {
                last = now;
                st.events.emit("control.changed", super::api::workers_snapshot(&st));
            }
        }
    });
}

// Emits docs.changed; the watch-mode compile trigger hooks in here.
pub fn spawn_docs_watcher(st: SharedState) {
    std::thread::spawn(move || {
        use notify::Watcher;
        let fingerprints = |st: &SharedState| -> std::collections::BTreeMap<String, String> {
            st.proj()
                .doc_files()
                .iter()
                .map(|f| {
                    let rel = super::api::rel_doc(&st.proj().root, f);
                    let fp = std::fs::metadata(f)
                        .map(|m| format!("{}:{:?}", m.len(), m.modified().ok()))
                        .unwrap_or_default();
                    (rel, fp)
                })
                .collect()
        };
        let mut last = fingerprints(&st);
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let mut watcher = match notify::recommended_watcher(move |res: Result<notify::Event, _>| {
            if res.is_ok() {
                let _ = tx.send(());
            }
        }) {
            Ok(w) => w,
            Err(_) => return, // no watcher available: docs.changed simply never fires
        };
        if watcher.watch(&st.proj().root, notify::RecursiveMode::Recursive).is_err() {
            return;
        }
        loop {
            if rx.recv().is_err() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
            while rx.try_recv().is_ok() {}
            let now = fingerprints(&st);
            if now == last {
                continue;
            }
            let mut changed: Vec<String> = Vec::new();
            for (rel, fp) in &now {
                if last.get(rel) != Some(fp) {
                    changed.push(rel.clone());
                }
            }
            for rel in last.keys() {
                if !now.contains_key(rel) {
                    changed.push(rel.clone());
                }
            }
            last = now;
            let store = crate::store::Store::load(&st.out);
            let graph_stale = st.proj().doc_files().iter().any(|f| {
                let rel = super::api::rel_doc(&st.proj().root, f);
                match (std::fs::read_to_string(f), store.docs.get(&rel)) {
                    (Ok(text), Some(rec)) => crate::model::hash_hex(&text) != rec.content_hash,
                    (Ok(_), None) => true,
                    _ => false,
                }
            });
            st.events.emit("docs.changed", json!({ "docs": changed, "graphStale": graph_stale }));
            super::jobs_hook_on_docs_changed(&st);
        }
    });
}
