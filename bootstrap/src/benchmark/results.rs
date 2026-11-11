use serde_json::Value;
use std::collections::BTreeMap;

use super::*;

// Known results embedded at compile time; curation is manual.
// Mirrors docs/benchmark/benchmark.md#machine-wide-history.
const KNOWN_RESULTS: &str = include_str!("../../../docs/benchmark/known-results.yaml");

fn home_history_path() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(|h| {
        std::path::PathBuf::from(h)
            .join(".jazyk")
            .join("benchmarks")
            .join("history.yaml")
    })
}

// Append one run to the machine-wide history: grades outlive the project that
// produced them. Mirrors docs/benchmark/benchmark.md#machine-wide-history.
pub fn append_history(model: &str, base_url: &str, codec_reports: &[(String, Value)]) {
    let Some(path) = home_history_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        crate::vfs::create_dir_all(parent).ok();
    }
    let mut root: Value = crate::vfs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_norway::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({"runs": []}));
    let graded_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = serde_json::json!({
        "model": model,
        "baseUrl": base_url,
        "gradedAt": graded_at,
        "caseSetHash": case_set_hash(),
        "codecs": codec_reports.iter().cloned().collect::<BTreeMap<String, Value>>(),
    });
    if let Some(runs) = root["runs"].as_array_mut() {
        runs.push(entry);
    }
    if let Ok(text) = serde_norway::to_string(&root) {
        crate::vfs::write(&path, text).ok();
    }
}

// The merged comparison table: embedded known results, the machine-wide history
// (latest per model and codec), and the project's own results file. Every entry
// carries its source and whether its caseSetHash matches this binary's.
pub fn all_results(out: &std::path::Path) -> Value {
    let current_hash = case_set_hash();
    let mut rows: Vec<Value> = Vec::new();
    let mut push =
        |model: &str, base_url: &str, graded_at: u64, hash: &str, codecs: &Value, source: &str| {
            rows.push(serde_json::json!({
                "model": model,
                "baseUrl": base_url,
                "gradedAt": graded_at,
                "caseSetHash": hash,
                "current": hash == current_hash,
                "codecs": codecs,
                "source": source,
            }));
        };
    if let Ok(known) = serde_norway::from_str::<Value>(KNOWN_RESULTS) {
        for e in known["results"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or(&[])
        {
            push(
                e["model"].as_str().unwrap_or("?"),
                e["baseUrl"].as_str().unwrap_or(""),
                e["gradedAt"].as_u64().unwrap_or(0),
                e["caseSetHash"].as_str().unwrap_or(""),
                &e["codecs"],
                "embedded",
            );
        }
    }
    if let Some(path) = home_history_path() {
        if let Some(hist) = crate::vfs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_norway::from_str::<Value>(&s).ok())
        {
            // Latest per (model, caseSetHash): history is append-only, the table shows tips.
            let mut latest: BTreeMap<(String, String), &Value> = BTreeMap::new();
            for e in hist["runs"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
                let key = (
                    e["model"].as_str().unwrap_or("?").to_string(),
                    e["caseSetHash"].as_str().unwrap_or("").to_string(),
                );
                let newer = latest
                    .get(&key)
                    .map(|p| p["gradedAt"].as_u64() <= e["gradedAt"].as_u64())
                    .unwrap_or(true);
                if newer {
                    latest.insert(key, e);
                }
            }
            for e in latest.values() {
                push(
                    e["model"].as_str().unwrap_or("?"),
                    e["baseUrl"].as_str().unwrap_or(""),
                    e["gradedAt"].as_u64().unwrap_or(0),
                    e["caseSetHash"].as_str().unwrap_or(""),
                    &e["codecs"],
                    "history",
                );
            }
        }
    }
    let project = crate::layout::benchmark_dir(out).join("results.yaml");
    if let Some(v) = crate::vfs::read_to_string(&project)
        .ok()
        .and_then(|s| serde_norway::from_str::<Value>(&s).ok())
    {
        if let Some(map) = v.as_object() {
            for (model, e) in map {
                push(
                    model,
                    "",
                    e["gradedAt"].as_u64().unwrap_or(0),
                    e["caseSetHash"].as_str().unwrap_or(""),
                    &e["codecs"],
                    "project",
                );
            }
        }
    }
    rows.sort_by(|a, b| b["gradedAt"].as_u64().cmp(&a["gradedAt"].as_u64()));
    serde_json::json!({"caseSetHash": current_hash, "results": rows})
}

// One entry per model in the set's log directory, benchmark/results.yaml, updated in place. Mirrors
// docs/benchmark/benchmark.md#results-file.
// A tier score is a number only when at least one of its cases ran; a filtered
// run leaves the rest null instead of a fake 0.0.
pub(super) fn score_json(
    t: &str,
    measured: &dyn Fn(&str) -> bool,
    tier_score: &dyn Fn(&str) -> f64,
) -> Value {
    if measured(t) {
        serde_json::json!((tier_score(t) * 100.0).round() / 100.0)
    } else {
        Value::Null
    }
}

pub(super) fn write_results(out: &std::path::Path, model: &str, codec_reports: &[(String, Value)]) {
    let path = crate::layout::benchmark_dir(out).join("results.yaml");
    let mut all: BTreeMap<String, Value> = crate::vfs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_norway::from_str(&s).ok())
        .unwrap_or_default();
    let graded_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    all.insert(
        model.to_string(),
        serde_json::json!({
            "gradedAt": graded_at,
            "caseSetHash": case_set_hash(),
            "codecs": codec_reports.iter().cloned().collect::<BTreeMap<String, Value>>(),
        }),
    );
    if crate::vfs::create_dir_all(path.parent().unwrap()).is_ok() {
        match serde_norway::to_string(&all) {
            Ok(y) => {
                if let Err(e) = crate::vfs::write(&path, y) {
                    eprintln!("jazyk: could not write {}: {}", path.display(), e);
                } else {
                    println!("\nresults: {}", path.display());
                }
            }
            Err(e) => eprintln!("jazyk: could not serialize results: {}", e),
        }
    }
}
