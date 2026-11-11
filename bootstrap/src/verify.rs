pub mod artifacts;
use crate::vfs::VPathExt;
// Verification: run the tests the ledger records and feed verdicts back. Status is
// derived, never stored, from byte hashes and current Source membership. Mirrors
// docs/consumers/gen.md (the ledger, runners) and
// docs/compiler/tools.md#verification-tools.
use crate::gen::{artifact_path, hash_file, GenSettings, Ledger, ReqRow};
use crate::model::hash_hex;
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

mod audit;
#[cfg(test)]
mod audit_tests;
mod judge;
#[cfg(test)]
mod judge_tests;
mod mark;
mod runners;
#[cfg(test)]
mod runners_tests;
mod select;
#[cfg(test)]
mod select_tests;
mod status;
#[cfg(test)]
mod status_tests;
#[cfg(test)]
mod tests;

pub use audit::audit;
pub use judge::task;
pub(crate) use judge::{judge_files, judge_input, JUDGE};
use judge::{judge_llm_criteria, parse_verdict, record_llm_failure, JudgmentError};
pub(crate) use mark::rebaseline_files;
pub use mark::{mark, mark_llm};
pub(crate) use runners::run_test_ref;
use runners::task_case_outcome;
pub use runners::{record_runner_failure, run_criterion_tests, run_programmatic, ProgRun};
pub use select::{run_all, run_selected, select_rows};
pub(crate) use select::{run_selected_after_build, run_without_build};
pub(crate) use status::artifact_names;
use status::ensure_current_source;
pub use status::{
    gate, pending, pending_counts, questioned, repair_for, status_map, status_of, status_with,
    status_word,
};

// UTC ISO 8601 from the system clock, no dependencies. Civil-from-days per Howard
// Hinnant's algorithm.
pub fn now_iso() -> String {
    let secs = crate::sys::now_unix() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mth <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mth, d, h, m, s)
}
