// The SSE replay ring at its edges, and the stream a reconnecting client gets.
// Mirrors docs/frontends/gui.md#events.
use super::*;
use axum::response::IntoResponse;

fn seqs(events: &[Arc<Value>]) -> Vec<u64> {
    events.iter().map(|e| e["seq"].as_u64().unwrap()).collect()
}

fn id(hub: &EventHub, seq: u64) -> String {
    format!("{}-{}", hub.inner.boot, seq)
}

// Past the ring's size the oldest events go first; the ring keeps the newest RING.
#[test]
fn the_ring_evicts_the_oldest_and_keeps_the_newest() {
    let hub = EventHub::new();
    for _ in 0..RING + 10 {
        hub.emit("x", json!({}));
    }
    let ring = hub.inner.ring.lock().unwrap();
    assert_eq!(ring.len(), RING);
    assert_eq!(ring.front().unwrap()["seq"], 11);
    assert_eq!(ring.back().unwrap()["seq"], (RING + 10) as u64);
}

// A Last-Event-ID just before the oldest kept event replays the whole ring; one
// further back has lost events and resyncs, as does zero once anything was evicted.
// The newest replays nothing, the one before it the newest alone.
#[test]
fn replay_covers_exactly_the_ring_and_resyncs_past_it() {
    let hub = EventHub::new();
    let total = (RING + 10) as u64;
    for _ in 0..total {
        hub.emit("x", json!({}));
    }
    let oldest = 11;
    let all = hub.replay_after_id(&id(&hub, oldest - 1)).unwrap();
    assert_eq!(all.len(), RING);
    assert_eq!(seqs(&all).first(), Some(&oldest));
    assert_eq!(seqs(&all).last(), Some(&total));
    assert!(hub.replay_after_id(&id(&hub, oldest - 2)).is_none());
    assert!(hub.replay_after_id(&id(&hub, 0)).is_none());
    let tail = hub.replay_after_id(&id(&hub, oldest)).unwrap();
    assert_eq!(tail.len(), RING - 1);
    assert!(hub.replay_after_id(&id(&hub, total)).unwrap().is_empty());
    assert_eq!(
        seqs(&hub.replay_after_id(&id(&hub, total - 1)).unwrap()),
        vec![total]
    );
    // One past the newest is an id this server never issued.
    assert!(hub.replay_after_id(&id(&hub, total + 1)).is_none());
}

// Before any event, the client that saw nothing resumes with nothing to replay;
// any later id is ahead of the server. Malformed ids and another boot's ids resync.
#[test]
fn an_empty_ring_and_foreign_or_malformed_ids() {
    let hub = EventHub::new();
    assert!(hub.replay_after_id(&id(&hub, 0)).unwrap().is_empty());
    assert!(hub.replay_after_id(&id(&hub, 1)).is_none());
    hub.emit("x", json!({}));
    assert_eq!(
        seqs(&hub.replay_after_id(&id(&hub, 0)).unwrap()),
        vec![1],
        "nothing evicted yet: zero replays everything"
    );
    let boot = hub.inner.boot;
    for bad in [
        format!("{}-1", boot.wrapping_sub(1)),
        format!("{}-1", boot + 1),
        format!("{}-x", boot),
        format!("{}-1-2", boot),
        format!("{}--1", boot),
        format!("-{}", 1),
        "abc-1".to_string(),
        "1".to_string(),
        String::new(),
    ] {
        assert!(hub.replay_after_id(&bad).is_none(), "{bad}");
    }
}

// Scoped hubs share one sequence and one ring; a scoped event names its set.
#[test]
fn scoped_hubs_share_the_sequence_and_name_their_set() {
    let hub = EventHub::new();
    let a = hub.scoped(Some("ds_a".into()));
    let b = hub.scoped(Some("ds_b".into()));
    a.emit("store.generation", json!({"generation": 1}));
    b.emit("store.generation", json!({"generation": 2}));
    hub.emit("sets.changed", json!({}));
    let all = hub.replay_after_id(&id(&hub, 0)).unwrap();
    assert_eq!(seqs(&all), vec![1, 2, 3]);
    assert_eq!(all[0]["setId"], "ds_a");
    assert_eq!(all[1]["setId"], "ds_b");
    assert!(all[2].get("setId").is_none());
    assert_eq!(all[0]["type"], "store.generation");
    // An id issued through one scope resumes through another.
    assert_eq!(seqs(&b.replay_after_id(&id(&a, 1)).unwrap()), vec![2, 3]);
}

// The data frames of an SSE body, as (id, payload), read until `n` arrived.
async fn frames(body: axum::body::Body, n: usize) -> Vec<(Option<String>, Value)> {
    let mut stream = body.into_data_stream();
    let mut text = String::new();
    let mut out = Vec::new();
    while out.len() < n {
        let chunk = tokio::time::timeout(std::time::Duration::from_secs(60), stream.next())
            .await
            .expect("the stream stalled")
            .expect("the stream ended")
            .unwrap();
        text.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(end) = text.find("\n\n") {
            let frame: String = text.drain(..end + 2).collect();
            let mut id = None;
            let mut data = None;
            for line in frame.lines() {
                if let Some(v) = line.strip_prefix("id:") {
                    id = Some(v.trim().to_string());
                } else if let Some(v) = line.strip_prefix("data:") {
                    data = Some(serde_json::from_str::<Value>(v.trim()).unwrap());
                }
            }
            if let Some(data) = data {
                out.push((id, data));
            }
        }
    }
    out
}

async fn open(st: &SharedState, last_event_id: Option<&str>) -> axum::body::Body {
    let mut headers = HeaderMap::new();
    if let Some(v) = last_event_id {
        headers.insert("last-event-id", v.parse().unwrap());
    }
    sse(State(st.clone()), headers)
        .await
        .into_response()
        .into_body()
}

// A reconnect inside the ring gets the missed events with their ids, then live
// ones; an id from another boot opens with `resync`; no id opens live.
#[tokio::test]
async fn the_stream_replays_missed_events_then_goes_live() {
    let st = super::super::server::tests::state(None, "https://events-replay.test");
    for n in 1..=3 {
        st.events.emit("x", json!({"n": n}));
    }
    let boot = st.events.inner.boot;
    let body = open(&st, Some(&format!("{}-1", boot))).await;
    st.events.emit("x", json!({"n": 4}));
    let got = frames(body, 3).await;
    assert_eq!(
        got.iter().map(|(_, v)| v["n"].clone()).collect::<Vec<_>>(),
        vec![json!(2), json!(3), json!(4)]
    );
    assert_eq!(got[0].0.as_deref(), Some(format!("{}-2", boot).as_str()));
    assert_eq!(got[2].0.as_deref(), Some(format!("{}-4", boot).as_str()));

    let body = open(&st, Some(&format!("{}-1", boot + 1))).await;
    st.events.emit("x", json!({"n": 5}));
    let got = frames(body, 2).await;
    assert_eq!(got[0].1, json!({"type": "resync"}));
    assert_eq!(got[0].0, None, "a resync carries no id to resume from");
    assert_eq!(got[1].1["n"], 5);

    let body = open(&st, None).await;
    st.events.emit("x", json!({"n": 6}));
    let got = frames(body, 1).await;
    assert_eq!(got[0].1["n"], 6);
    std::fs::remove_dir_all(&st.root).ok();
}

// A consumer that fell further behind than the live channel holds is told to
// refetch: `resync`, then the events the channel still had.
#[tokio::test]
async fn a_lagging_consumer_gets_resync() {
    let st = super::super::server::tests::state(None, "https://events-lag.test");
    let body = open(&st, None).await;
    for n in 0..300 {
        st.events.emit("x", json!({"n": n}));
    }
    let got = frames(body, 2).await;
    assert_eq!(got[0].1, json!({"type": "resync"}));
    assert!(got[1].1["n"].as_u64().unwrap() > 0, "{:?}", got[1]);
    std::fs::remove_dir_all(&st.root).ok();
}
