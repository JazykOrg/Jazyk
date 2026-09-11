// Conversations that outlive the process: one JSON-lines file per chat session under
// `<out>/sessions/`, the same shape a job trace uses (a metadata first line, then
// records). The store is per project because a conversation is about a project, and
// it sits beside the graph it discusses. Mirrors docs/frontends/acp.md#session-store.
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

pub struct SessionMeta {
    pub id: String,
    pub title: String,
    pub started_at: String,
    pub updated_at: String,
    pub turns: u64,
}

fn dir(out: &Path) -> PathBuf {
    out.join("sessions")
}

fn path_for(out: &Path, id: &str) -> PathBuf {
    // Ids come from an agent, so they are file names only after sanitizing.
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    dir(out).join(format!("{}.jsonl", safe))
}

// Timestamps are ISO 8601 because that is what a session listing carries on the
// wire, and clients render them as "5m ago". Mirrors docs/frontends/acp.md#session-store.
fn now() -> String {
    crate::verify::now_iso()
}

// Start recording a conversation. Repeated opens of the same id append to it, so a
// resumed session keeps one file.
pub fn open(out: &Path, id: &str, cwd: &Path, agent: &str) {
    let path = path_for(out, id);
    if path.exists() {
        return;
    }
    std::fs::create_dir_all(dir(out)).ok();
    let meta = json!({"kind": "meta", "id": id, "cwd": cwd.display().to_string(),
                      "agent": agent, "startedAt": now()});
    if let Ok(mut f) = std::fs::File::create(&path) {
        let _ = writeln!(f, "{}", meta);
    }
}

pub fn append(out: &Path, id: &str, record: Value) {
    let path = path_for(out, id);
    if !path.exists() {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&path) {
        let _ = writeln!(f, "{}", record);
    }
}

// The first thing a person said, which is what a history list should show. Recorded
// once: a conversation keeps the name it opened with.
pub fn record_prompt(out: &Path, id: &str, text: &str) {
    append(out, id, json!({"kind": "user", "at": now(), "text": text}));
}

pub fn record_update(out: &Path, id: &str, update: &Value) {
    append(
        out,
        id,
        json!({"kind": "update", "at": now(), "update": update}),
    );
}

// Every recorded conversation for this project, newest first.
pub fn list(out: &Path) -> Vec<SessionMeta> {
    let mut sessions: Vec<SessionMeta> = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir(out)) else {
        return sessions;
    };
    for e in rd.flatten() {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        let mut id = String::new();
        let mut started = String::new();
        let mut updated = String::new();
        let mut title = String::new();
        let mut turns = 0u64;
        for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match v["kind"].as_str().unwrap_or("") {
                "meta" => {
                    id = v["id"].as_str().unwrap_or_default().to_string();
                    started = v["startedAt"].as_str().unwrap_or_default().to_string();
                }
                "user" => {
                    turns += 1;
                    if title.is_empty() {
                        title = summarize(v["text"].as_str().unwrap_or_default());
                    }
                    updated = v["at"].as_str().unwrap_or_default().to_string();
                }
                _ => {
                    if let Some(at) = v["at"].as_str() {
                        updated = at.to_string();
                    }
                }
            }
        }
        if id.is_empty() {
            continue;
        }
        if updated.is_empty() {
            updated = started.clone();
        }
        if title.is_empty() {
            title = "(no prompt yet)".to_string();
        }
        sessions.push(SessionMeta {
            id,
            title,
            started_at: started,
            updated_at: updated,
            turns,
        });
    }
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    sessions
}

// How many turns a conversation already holds. Distinguishes the opening prompt,
// which is what names the conversation.
pub fn turns(out: &Path, id: &str) -> u64 {
    read(out, id).iter().filter(|r| r["kind"] == "user").count() as u64
}

// One conversation's records, in order.
pub fn read(out: &Path, id: &str) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(path_for(out, id)) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

// A conversation's name is its opening line, shortened. A picker shows a list of
// these, so a whole paragraph in the title is worse than nothing.
fn summarize(text: &str) -> String {
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    let mut short: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        short.push('…');
    }
    short
}

// Conversations a project no longer needs. Kept simple on purpose: the newest `keep`
// survive, the rest are removed, and the caller decides when to run it.
pub fn prune(out: &Path, keep: usize) -> usize {
    let all = list(out);
    let mut removed = 0;
    for meta in all.into_iter().skip(keep) {
        if std::fs::remove_file(path_for(out, &meta.id)).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    // A conversation survives the process that held it: what was said comes back in
    // order, titled by its opening line and dated by its last activity.
    // Mirrors docs/frontends/acp.md#session-store.
    #[test]
    fn a_conversation_is_written_read_and_listed() {
        let out = std::env::temp_dir().join(format!("jazyk-sess-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let cwd = std::path::Path::new("/tmp/project");

        open(&out, "sess-1", cwd, "embedded");
        record_prompt(
            &out,
            "sess-1",
            "what does the payment document promise?\nsecond line",
        );
        record_update(
            &out,
            "sess-1",
            &json!({"sessionUpdate": "agent_message_chunk"}),
        );
        // Reopening an existing conversation appends rather than truncating.
        open(&out, "sess-1", cwd, "embedded");
        record_prompt(&out, "sess-1", "and the refund window?");

        // One header, then everything said, in order: reopening appended.
        let records = read(&out, "sess-1");
        assert_eq!(records.len(), 4, "{:?}", records);
        assert_eq!(records[0]["kind"], "meta");
        assert_eq!(
            records[1]["text"],
            "what does the payment document promise?\nsecond line"
        );
        assert_eq!(records[3]["text"], "and the refund window?");

        let listed = list(&out);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "sess-1");
        assert_eq!(listed[0].turns, 2);
        assert_eq!(listed[0].title, "what does the payment document promise?");

        // An id that names a path cannot escape the store.
        open(&out, "../escape", cwd, "embedded");
        assert!(!out
            .parent()
            .map(|p| p.join("escape.jsonl").exists())
            .unwrap_or(false));
        assert!(list(&out).iter().any(|m| m.id == "../escape"));

        assert_eq!(read(&out, "nope").len(), 0);
        std::fs::remove_dir_all(&out).ok();
    }

    // Pruning keeps the conversations a person is most likely to want.
    #[test]
    fn pruning_keeps_the_newest() {
        let out = std::env::temp_dir().join(format!("jazyk-sess-prune-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let cwd = std::path::Path::new("/tmp/project");
        for (i, at) in [("a", "100"), ("b", "200"), ("c", "300")] {
            open(&out, i, cwd, "embedded");
            append(
                &out,
                i,
                json!({"kind": "user", "at": at, "text": format!("prompt {}", i)}),
            );
        }
        assert_eq!(prune(&out, 2), 1);
        let left: Vec<String> = list(&out).into_iter().map(|m| m.id).collect();
        assert_eq!(left, vec!["c".to_string(), "b".to_string()]);
        std::fs::remove_dir_all(&out).ok();
    }
}
