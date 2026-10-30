//! Durable command cursors around the ledger workers. GDK owns session progress.
use crate::runner::{BatchRun, SessionReport};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

pub(crate) fn enabled() -> bool {
    crate::session_recovery::active() && (cfg!(target_arch = "wasm32") || crate::vfs::is_scoped())
}

pub(crate) struct Progress<T> {
    key: String,
    signature: Value,
    pub data: T,
}

impl<T: Serialize + DeserializeOwned> Progress<T> {
    pub fn load(key: String, signature: Value, fresh: impl FnOnce() -> T) -> Result<Self, String> {
        let value = if enabled() {
            crate::session_recovery::parent(&key)?
        } else {
            None
        };
        let data = match value {
            Some(value) => {
                if value["version"] != 1 || value["signature"] != signature {
                    return Err(
                        "checkpoint-incompatible: ledger command version or arguments changed"
                            .into(),
                    );
                }
                serde_json::from_value(value["data"].clone())
                    .map_err(|error| format!("checkpoint-incompatible: ledger command: {error}"))?
            }
            None => fresh(),
        };
        Ok(Self {
            key,
            signature,
            data,
        })
    }

    #[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
    pub async fn save(&self) -> Result<(), String> {
        if !enabled() {
            return Ok(());
        }
        crate::session_recovery::set_parent(
            &self.key,
            serde_json::json!({
                "version": 1, "signature": self.signature, "data": self.data,
            }),
        );
        crate::session_recovery::checkpoint_parent()
            .await
            .map_err(|error| {
                if crate::session_recovery::is_interruption(&error) {
                    error
                } else {
                    format!("checkpoint-failed: {error}")
                }
            })
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct BatchProgress {
    pub batch: BatchRun,
    pub generation: u64,
    pub report: Option<SessionReport>,
}

impl BatchProgress {
    pub fn new(batch: BatchRun, out: &Path) -> Self {
        Self {
            batch,
            generation: crate::store::read_generation(out),
            report: None,
        }
    }

    pub fn committed_report(&self, out: &Path) -> Option<SessionReport> {
        // Share the compiler's journal proof rather than invent a second success
        // signal for ledger work. The worker's gates remain authoritative.
        crate::reconcile::continuation::ActiveBatch {
            run: self.batch.clone(),
            class: self.batch.class(),
            tier: None,
            generation: self.generation,
            records: Default::default(),
            report: None,
        }
        .committed_report(out)
    }

    pub fn finish(&self, out: &Path) {
        crate::session_recovery::remove_frame(&crate::session_recovery::batch_key(
            out,
            &self.batch.id,
        ));
    }
}

pub(crate) fn interruption(report: &SessionReport) -> Option<String> {
    report
        .stopped
        .iter()
        .chain(report.failed.iter())
        .find(|reason| crate::session_recovery::is_interruption(reason))
        .cloned()
}

pub(crate) fn command_key(kind: &str, out: &Path, signature: &Value) -> String {
    let digest = crate::model::hash_hex(&signature.to_string());
    crate::session_recovery::command_key(&format!("{kind}:{}:{digest}", out.display()))
}
