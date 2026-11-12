//! Host records: engine data that is a log, not project state (conversations, feedback,
//! transcripts), kept by the host in a record store of its own, never in the workspace
//! snapshot and never in git. The store knows nothing of databases: named streams of
//! JSON records. Without a store a hosted run keeps no conversations and no feedback.
//! Mirrors docs/compiler/hosted-runtime.md#host-records.
use serde_json::Value;
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

/// The error a request that needs the host's record store answers without one.
pub const NO_RECORDS: &str = "this host keeps no conversation records";

/// The record store this request uses: the host's, carried by its execution. Without
/// one the request keeps no conversations and no feedback; nothing falls back to files
/// in the workspace snapshot.
pub fn store() -> Result<Arc<dyn RecordStore>, String> {
    super::execution::record_store().ok_or_else(|| NO_RECORDS.to_string())
}
