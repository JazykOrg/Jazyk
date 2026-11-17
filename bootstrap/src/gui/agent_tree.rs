// The agent list the editor follows: the running build's sessions, then the
// conversations and their sub-agents. `GET /api/agent-tree` answers it and the
// `agents` event carries it whenever it changes. Mirrors docs/frontends/gui.md#agents.
use super::state::SharedState;
use axum::{extract::State, response::IntoResponse, response::Response, Json};
use serde_json::{json, Value};
use std::collections::BTreeMap;

// The idle conversations the list keeps beside the running ones.
pub(super) const RECENT_IDLE: usize = 20;

// Build sessions of one job, from its trace: a session opens at `sessionStart`,
// moves with `section`, and closes with its terminal event. Several run at once.
pub(super) fn build_rows<'a>(
    job: u64,
    kind: &Value,
    events: impl IntoIterator<Item = &'a Value>,
) -> Vec<Value> {
    let title = kind["kind"]
        .as_str()
        .or_else(|| kind.as_str())
        .unwrap_or("build")
        .to_string();
    let mut open: BTreeMap<String, Value> = BTreeMap::new();
    for payload in events {
        let event = &payload["event"];
        let Some(label) = event["label"].as_str() else {
            continue;
        };
        match event["kind"].as_str() {
            Some("sessionStart") => {
                let mut location = Value::Null;
                if let Some(doc) = event["doc"].as_str() {
                    location = json!({"path": doc});
                    if let Some(section) = event["sections"][0].as_str() {
                        location["section"] = json!(section);
                    }
                }
                let task = [event["task"].as_str(), event["target"].as_str()]
                    .into_iter()
                    .flatten()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                open.insert(
                    label.to_string(),
                    json!({"task": task, "location": location}),
                );
            }
            Some("section") => {
                if let Some(row) = open.get_mut(label) {
                    row["location"] = json!({"path": event["doc"], "section": event["section"]});
                }
            }
            Some("sessionDone" | "sessionFailed" | "sessionEnded") => {
                open.remove(label);
            }
            _ => {}
        }
    }
    let row = |id: String, title: String, task: Value, location: Value| {
        let mut row = json!({"id": id, "agentId": id, "kind": "build", "jobId": job,
            "parentId": null, "rootId": id, "depth": 0, "title": title, "task": task,
            "state": "running"});
        if !location.is_null() {
            row["location"] = location;
        }
        row
    };
    if open.is_empty() {
        return vec![row(format!("{job}:build"), title, Value::Null, Value::Null)];
    }
    open.into_iter()
        .map(|(label, s)| {
            row(
                format!("{job}:{label}"),
                format!("{title} {label}"),
                s["task"].clone(),
                s["location"].clone(),
            )
        })
        .collect()
}

pub(super) fn rows(st: &SharedState) -> Vec<Value> {
    let mut out = st
        .jobs
        .running_job()
        .and_then(|job| {
            st.jobs
                .with_job(job, |kind, events| build_rows(job, kind, events))
        })
        .unwrap_or_default();
    out.extend(st.chat.agent_rows());
    out
}

pub(super) fn emit(st: &SharedState) {
    st.events.emit("agents", json!({"agents": rows(st)}));
}

// The conversation list changed: the chat pane's list and the agent list follow.
pub(super) fn sessions_changed(st: &SharedState) {
    st.events
        .emit("chat.sessions", json!({"sessions": st.chat.snapshot()}));
    emit(st);
}

// Whether a job trace event opens, moves, or closes a build session row.
pub(super) fn moves_rows(event: &Value) -> bool {
    matches!(
        event["kind"].as_str(),
        Some("sessionStart" | "section" | "sessionDone" | "sessionFailed" | "sessionEnded")
    )
}

pub async fn agent_tree(State(st): State<SharedState>) -> Response {
    let rows = tokio::task::spawn_blocking(move || rows(&st))
        .await
        .unwrap_or_default();
    Json(json!({"agents": rows})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Concurrent sessions of one build are one row each; a closed one leaves.
    #[test]
    fn build_sessions_are_rows_until_they_end() {
        let start = |label: &str, doc: &str| json!({"event":{"kind":"sessionStart","label":label,"task":"reconcile-section","target":doc,"doc":doc,"sections":["docs/a.md#/intro"]}});
        let events = vec![
            start("b1", "docs/a.md"),
            start("b2", "docs/b.md"),
            json!({"event":{"kind":"section","label":"b2","doc":"docs/b.md","section":"docs/b.md#/two"}}),
            start("b3", "docs/c.md"),
            json!({"event":{"kind":"sessionDone","label":"b1"}}),
        ];
        let rows = build_rows(7, &json!({"kind":"compile"}), &events);
        let ids: Vec<&str> = rows.iter().filter_map(|r| r["id"].as_str()).collect();
        assert_eq!(ids, vec!["7:b2", "7:b3"]);
        assert_eq!(rows[0]["kind"], "build");
        assert_eq!(rows[0]["jobId"], 7);
        assert_eq!(rows[0]["rootId"], "7:b2");
        assert_eq!(rows[0]["state"], "running");
        assert_eq!(rows[0]["title"], "compile b2");
        assert_eq!(rows[0]["location"]["section"], "docs/b.md#/two");
        let idle = build_rows(8, &json!("check"), &Vec::new());
        assert_eq!(idle[0]["id"], "8:build");
        assert!(idle[0].get("location").is_none());
    }
}
