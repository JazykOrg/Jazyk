//! Host records: engine data that is a log, not project state (conversations, feedback,
//! transcripts), kept by the host in a record store of its own, never in the workspace
//! snapshot and never in git. The store knows nothing of databases: named streams of
//! JSON records. Without a store the workspace keeps the streams as files under
//! `.jazyk/local/records/`. Mirrors docs/compiler/hosted-runtime.md#host-records.
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

/// A named stream of records, keyed by conversation id, set id, or run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    /// A conversation's numbered timeline, the updates the editor shows.
    ChatUpdates,
    /// The messages the model reads, in generations: compaction or a stop restore
    /// replaces them under the next generation.
    ChatHistory,
    /// One set's `report_feedback` entries.
    Feedback,
    /// One run's transcript.
    Trace,
}

impl Stream {
    pub fn name(self) -> &'static str {
        match self {
            Stream::ChatUpdates => "chat-updates",
            Stream::ChatHistory => "chat-history",
            Stream::Feedback => "feedback",
            Stream::Trace => "trace",
        }
    }
}

/// The host's record store. Every operation is keyed by stream and key; records are
/// numbered from 0 within a key. The engine requires only that a stream it reads back
/// holds what it appended, in order, and treats a missing key as empty.
pub trait RecordStore: Send + Sync {
    /// Every key of a stream with its metadata.
    fn keys(&self, stream: Stream) -> Result<Vec<(String, Value)>, String>;
    /// One key's metadata, `None` when the key is absent.
    fn meta(&self, stream: Stream, key: &str) -> Result<Option<Value>, String>;
    /// Write one key's metadata, creating the key.
    fn put_meta(&self, stream: Stream, key: &str, meta: &Value) -> Result<(), String>;
    /// Append records numbered from `from`, under generation `generation`. A number the
    /// key already holds in that generation is skipped, so a retry never duplicates.
    fn append(
        &self,
        stream: Stream,
        key: &str,
        generation: u32,
        from: u64,
        items: &[Value],
    ) -> Result<(), String>;
    /// Replace a key's records with `items` under `generation`, dropping every older one.
    fn replace(
        &self,
        stream: Stream,
        key: &str,
        generation: u32,
        items: &[Value],
    ) -> Result<(), String>;
    /// The records of the key's newest generation numbered above `after` (all when
    /// `None`), at most `limit`, in order.
    fn read(
        &self,
        stream: Stream,
        key: &str,
        after: Option<u64>,
        limit: usize,
    ) -> Result<Vec<(u64, Value)>, String>;
    /// Delete a key and its records.
    fn delete(&self, stream: Stream, key: &str) -> Result<(), String>;
}

/// The file-backed record store: one JSON file per key under `<dir>/<stream>/`, the
/// form a host without a store, a test, and the fallback all use.
pub struct FileRecords {
    dir: PathBuf,
}

impl FileRecords {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, stream: Stream, key: &str) -> PathBuf {
        self.dir
            .join(stream.name())
            .join(format!("{}.json", crate::store::node_stem(key).0))
    }

    fn load(&self, stream: Stream, key: &str) -> Result<Option<Value>, String> {
        match crate::vfs::read_to_string(self.path(stream, key)) {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|e| format!("record {}/{}: {}", stream.name(), key, e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn save(&self, stream: Stream, key: &str, doc: &Value) -> Result<(), String> {
        let path = self.path(stream, key);
        if let Some(parent) = path.parent() {
            crate::vfs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        crate::vfs::write(&path, doc.to_string()).map_err(|e| e.to_string())
    }

    fn doc(&self, stream: Stream, key: &str) -> Result<Value, String> {
        Ok(self
            .load(stream, key)?
            .unwrap_or_else(|| json!({"key": key, "meta": {}, "generation": 0, "items": []})))
    }
}

impl RecordStore for FileRecords {
    fn keys(&self, stream: Stream) -> Result<Vec<(String, Value)>, String> {
        let dir = self.dir.join(stream.name());
        let Ok(rd) = crate::vfs::read_dir(&dir) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.ends_with(".json") || name.starts_with('.') {
                continue;
            }
            let text = crate::vfs::read_to_string(e.path()).map_err(|e| e.to_string())?;
            let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {}", name, e))?;
            if let Some(key) = doc["key"].as_str() {
                out.push((key.to_string(), doc["meta"].clone()));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    fn meta(&self, stream: Stream, key: &str) -> Result<Option<Value>, String> {
        Ok(self.load(stream, key)?.map(|d| d["meta"].clone()))
    }

    fn put_meta(&self, stream: Stream, key: &str, meta: &Value) -> Result<(), String> {
        let mut doc = self.doc(stream, key)?;
        doc["meta"] = meta.clone();
        self.save(stream, key, &doc)
    }

    fn append(
        &self,
        stream: Stream,
        key: &str,
        generation: u32,
        from: u64,
        items: &[Value],
    ) -> Result<(), String> {
        let mut doc = self.doc(stream, key)?;
        let current = doc["generation"].as_u64().unwrap_or(0) as u32;
        if generation > current {
            doc["generation"] = json!(generation);
            doc["items"] = json!([]);
        } else if generation < current {
            return Ok(());
        }
        let list = doc["items"].as_array_mut().expect("items is a list");
        let have: std::collections::BTreeSet<u64> =
            list.iter().filter_map(|i| i[0].as_u64()).collect();
        for (k, item) in items.iter().enumerate() {
            let n = from + k as u64;
            if !have.contains(&n) {
                list.push(json!([n, item]));
            }
        }
        list.sort_by_key(|i| i[0].as_u64().unwrap_or(0));
        self.save(stream, key, &doc)
    }

    fn replace(
        &self,
        stream: Stream,
        key: &str,
        generation: u32,
        items: &[Value],
    ) -> Result<(), String> {
        let mut doc = self.doc(stream, key)?;
        doc["generation"] = json!(generation);
        doc["items"] = Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(n, item)| json!([n as u64, item]))
                .collect(),
        );
        self.save(stream, key, &doc)
    }

    fn read(
        &self,
        stream: Stream,
        key: &str,
        after: Option<u64>,
        limit: usize,
    ) -> Result<Vec<(u64, Value)>, String> {
        let Some(doc) = self.load(stream, key)? else {
            return Ok(Vec::new());
        };
        Ok(doc["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| Some((i[0].as_u64()?, i[1].clone())))
            .filter(|(n, _)| after.is_none_or(|a| *n > a))
            .take(limit)
            .collect())
    }

    fn delete(&self, stream: Stream, key: &str) -> Result<(), String> {
        match crate::vfs::remove_file(self.path(stream, key)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// The record store this request uses: the host's when its execution carries one, else
/// the file-backed fallback under `.jazyk/local/records/` of the hosted workspace.
pub fn store() -> Arc<dyn RecordStore> {
    super::execution::record_store().unwrap_or_else(|| {
        Arc::new(FileRecords::new(
            crate::layout::project_local_dir(&super::root().join(".jazyk")).join("records"),
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_records_append_idempotently_and_replace_by_generation() {
        let dir = std::env::temp_dir().join(format!("jazyk-records-{}", crate::sys::process_id()));
        std::fs::remove_dir_all(&dir).ok();
        let s = FileRecords::new(&dir);
        s.put_meta(Stream::ChatUpdates, "c1", &json!({"title": "Hello"}))
            .unwrap();
        s.append(Stream::ChatUpdates, "c1", 0, 0, &[json!("a"), json!("b")])
            .unwrap();
        // A retry of the same numbers adds nothing; new numbers append.
        s.append(Stream::ChatUpdates, "c1", 0, 1, &[json!("b"), json!("c")])
            .unwrap();
        let all = s.read(Stream::ChatUpdates, "c1", None, 100).unwrap();
        assert_eq!(
            all.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(
            s.read(Stream::ChatUpdates, "c1", Some(0), 1).unwrap(),
            vec![(1, json!("b"))]
        );
        s.replace(Stream::ChatHistory, "c1", 1, &[json!("summary")])
            .unwrap();
        // An append under an older generation is ignored.
        s.append(Stream::ChatHistory, "c1", 0, 0, &[json!("stale")])
            .unwrap();
        assert_eq!(
            s.read(Stream::ChatHistory, "c1", None, 9).unwrap(),
            vec![(0, json!("summary"))]
        );
        assert_eq!(s.keys(Stream::ChatUpdates).unwrap()[0].1["title"], "Hello");
        s.delete(Stream::ChatUpdates, "c1").unwrap();
        assert!(s.meta(Stream::ChatUpdates, "c1").unwrap().is_none());
        assert!(s
            .read(Stream::Feedback, "missing", None, 9)
            .unwrap()
            .is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
