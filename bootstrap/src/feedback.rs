// The feedback log: what a model found ambiguous, wrong, or confusing about jazyk's own
// prompts and tools. Not the project's problem; jazyk's. One JSON line per call under
// the out directory, append-only, never read back by the compiler.
// Mirrors docs/compiler/tools.md#feedback-tool.
use serde::Serialize;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};

// The caller's references, so a record says who and what called it. Set by the harness
// that owns the session; empty fields are omitted from the record.
#[derive(Clone, Default)]
pub struct Caller {
    // "session" (a compilation session) or "mcp" (an external agent).
    pub source: String,
    // The goal kinds of the batch, or the MCP serving mode. Empty falls back to the
    // scope's own kinds.
    pub task: String,
    // The batch id, or the tool call's own name under MCP.
    pub target: String,
    // The goal ids of the open batch; empty when none is open. Empty falls back to
    // the scope's goal ids.
    pub batch: Vec<String>,
    pub model: String,
    pub codec: String,
    // The run's transcript name under <out>/trace, when the run leaves one.
    pub run: Option<String>,
    // The MCP client name reported at initialize.
    pub client: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub at: String,
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub subject: String,
    pub message: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub task: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub target: String,
    // The goal ids of the open batch, absent when none is open.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub batch: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub codec: String,
    pub generation: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
}

pub const KINDS: [&str; 5] = ["ambiguous", "wrong", "confusing", "missing", "other"];

// An unrecognized kind is recorded as `other`: the tool never bounces a model that is
// already telling us it is confused.
pub fn normalize_kind(raw: &str) -> String {
    let k = raw.trim().to_lowercase();
    if KINDS.contains(&k.as_str()) {
        k
    } else {
        "other".to_string()
    }
}

pub fn path(out: &Path) -> PathBuf {
    out.join("feedback.jsonl")
}

// Append one record. Best effort: a feedback write never fails a turn.
pub fn append(out: &Path, entry: &Entry) {
    // No out directory means no project (a bare session in a test or a probe); the
    // record has nowhere to live, and a relative path would litter the cwd.
    if out.as_os_str().is_empty() {
        return;
    }
    std::fs::create_dir_all(out).ok();
    let Ok(line) = serde_json::to_string(entry) else {
        return;
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path(out))
    {
        let _ = writeln!(f, "{}", line);
        let _ = f.flush();
    }
}

// The log newest first, capped. Malformed lines are skipped, not fatal: the file is
// appended by concurrent processes.
pub fn read(out: &Path, limit: usize) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(path(out)) else {
        return Vec::new();
    };
    let mut entries: Vec<Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect();
    entries.reverse();
    entries.truncate(limit);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_normalize_to_the_catalog() {
        assert_eq!(normalize_kind("Ambiguous"), "ambiguous");
        assert_eq!(normalize_kind("weird"), "other");
    }

    #[test]
    fn appended_lines_read_back_newest_first() {
        let dir = std::env::temp_dir().join(format!("jazyk-fb-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        for (i, msg) in ["first", "second"].iter().enumerate() {
            append(
                &dir,
                &Entry {
                    at: format!("2026-01-0{}T00:00:00Z", i + 1),
                    kind: "confusing".into(),
                    subject: "upsert_requirement".into(),
                    message: (*msg).into(),
                    source: "session".into(),
                    task: "reconcile-section".into(),
                    target: "b3-1".into(),
                    batch: vec!["g:reconcile-section:docs/cli.md#/cli".into()],
                    model: "m".into(),
                    codec: "native".into(),
                    generation: 3,
                    run: Some("r1".into()),
                    client: None,
                },
            );
        }
        let got = read(&dir, 10);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0]["message"], "second");
        assert_eq!(got[0]["run"], "r1");
        assert!(got[0].get("client").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
