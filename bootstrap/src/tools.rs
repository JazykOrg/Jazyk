use crate::vfs::VPathExt;
// The tool registry: the graph's only interface for models. One registry, served
// in-process to the turn harness and over stdio as the MCP server.
// Mirrors docs/compiler/tools.md.
use crate::context::LoadedSet;
use crate::model::*;
use crate::session::{SkillLoad, SkillState};
use crate::store::{Op, Store};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

mod anchors_coverage;
mod args;
mod catalog;
mod claims;
mod diagnostics;
mod entities;
mod files;
mod guard;
mod ledger;
mod read;
mod requirements;
mod scope;
mod session;
#[cfg(test)]
mod tests;
mod views;

use self::args::*;
use self::catalog::*;

pub(crate) use self::args::normalize_args;
pub use self::catalog::{
    catalog, toolset, toolset_for_kinds, ToolDef, BIND_TOOLS, CHAT_TOOLS, FEEDBACK_LIMIT,
    FEEDBACK_TOOL, FILE_TOOLS, GEN_TOOLS, GOAL_TOOLS, GRAPH_WRITE_TOOLS, PAIR_SETTLE_TOOL,
    PAIR_VERDICTS, READ_TOOLS, REVIEW_RULES, SPEND_TOOL, VERIFY_TOOLS, VIEW_TOOLS,
};
pub(crate) use self::catalog::{done_parameters, prompt_schema};
pub use self::scope::{harness_owned_rule, GoalOutcome, GoalScope, WorkScope};
pub use self::session::{opened_package, CallMemory, ToolSession};

// A rejection: names the violated rule and how to repair the call, because the caller is a
// model that will read it and try again.
#[derive(Debug)]
pub struct ToolError {
    pub rule: String,
    pub message: String,
}

impl ToolError {
    pub fn new(rule: &str, message: String) -> ToolError {
        ToolError {
            rule: rule.to_string(),
            message,
        }
    }
    pub fn to_value(&self) -> Value {
        json!({"error": {"rule": self.rule, "message": self.message}})
    }
}
