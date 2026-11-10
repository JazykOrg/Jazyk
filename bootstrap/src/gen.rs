use crate::vfs::VPathExt;
#[cfg(test)]
mod asset_workflow_tests;
pub(crate) mod assets;
#[cfg(test)]
mod concurrency_tests;
mod files;
mod ledger;
mod mark;
#[cfg(test)]
mod matcher_recovery_tests;
mod medium;
mod order;
mod pipeline;
pub(crate) mod progress;
mod settings;
mod steps;
mod task;
#[cfg(test)]
mod tests;
mod worker;
// Generation: the shared contract between every generation worker, the built-in
// `jazyk gen` and external MCP agents alike. One task per entity produces the entity's
// part of the deliverable and the tests for its requirements; the ledger binds them to
// the graph. Mirrors docs/consumers/gen.md and docs/compiler/tools.md#generation-tools.
use crate::model::hash_hex;
use crate::store::Store;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub use files::*;
pub use ledger::*;
pub use mark::*;
pub use medium::*;
pub use order::*;
pub use settings::*;
pub use steps::*;
pub use task::*;
pub use worker::*;
