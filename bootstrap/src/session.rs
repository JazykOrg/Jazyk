// The session harness: trace events, the transcript, the skill payloads, and the
// assembled session prompt for one goal batch. One agent, many goal kinds; variety
// lives in the kinds, which are data. Mirrors docs/compiler/sessions.md.
use crate::context::LoadedSet;
use crate::goals;
use crate::llm;
use crate::model::Goal;
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::BTreeSet;

mod prompt;
#[cfg(test)]
mod prompt_tests;
mod skills;
#[cfg(test)]
mod skills_tests;
mod trace;

pub(crate) use prompt::BLOCKS;
#[cfg(test)]
pub(crate) use prompt::GATES;
pub use prompt::{
    change_line, change_words, condense, full_payload, gate_line, initial_loaded, named_section,
    open_batch_prompt, preview, preview_with, provenance_line, refusal, session_prompt,
    session_prompt_elided, with_opening_contract, ProjectBlock, Protocol,
};
pub use skills::{skill_for_target, skill_payload, SkillEntry, SkillLoad, SkillState, SKILLS};
pub use trace::{Trace, TraceEvent, TraceLevel, TraceSink};
