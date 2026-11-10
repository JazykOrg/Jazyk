// The reconciler: compares the documents (desired state) against the graph (observed
// state), derives the goal board, and runs sessions one batch at a time until no goal
// is ready or the budget is spent. Then the deterministic tail: the checks, flip
// detection, rendering, docsgen, the verdict. Deterministic; the model never decides
// what is stale or what runs next. Mirrors docs/compiler/reconciler.md and
// docs/compiler/compilation.md.
use crate::board::Board;
use crate::control::Control;
use crate::derive::Indices;
use crate::goals;
use crate::llm::Llm;
use crate::md;
use crate::model::*;
use crate::project::Project;
use crate::session::{Trace, TraceEvent};
use crate::store::Store;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(crate) mod continuation;
use continuation::{ActiveBatch, Continuation, Receipt};

mod build_loop;
mod checks;
mod flips;
mod report;
mod ripple;
pub use build_loop::*;
pub use checks::*;
pub use flips::*;
pub use report::*;
pub use ripple::*;

// One deterministic finding: rule, subjects, severity, message, and the question the
// finding carries when its resolution is enumerable. Most checks name one subject;
// nondeterministic-transition names the pair.
// Every matched document parsed: path to its content hash and section tree.
pub type ParsedDocs = BTreeMap<String, (String, BTreeMap<String, Section>)>;

pub type Finding = (
    String,
    Vec<String>,
    String,
    String,
    Option<DiagnosticPrompt>,
);

fn finding(rule: &str, subject: &str, severity: &str, message: String) -> Finding {
    (
        rule.to_string(),
        vec![subject.to_string()],
        severity.to_string(),
        message,
        None,
    )
}

// Inventory selected files and project supported prose. Unreadable inputs retain
// their last tree; readiness still sees the read failure in the current inventory.
pub fn parse_all(proj: &Project) -> (ParsedDocs, BTreeMap<String, Vec<String>>) {
    let mut parsed = BTreeMap::new();
    let mut links = BTreeMap::new();
    for (rel, input) in crate::input::scan(proj) {
        match input {
            Ok((hash, sections)) => {
                let text = crate::input::projection_text(&sections);
                links.insert(rel.clone(), md::doc_links(&text, &rel));
                parsed.insert(rel, (hash, sections));
            }
            Err(_) => {
                if let Some(rec) = crate::store::stored_doc(&proj.out, &rel) {
                    parsed.insert(rel, (rec.content_hash, rec.sections));
                }
            }
        }
    }
    (parsed, links)
}

#[cfg(test)]
mod e2e;

#[cfg(test)]
mod test_support;
