use super::BuildReport;
use crate::model::{Costs, ParkedGoal};
use crate::runner::{BatchRun, SessionReport};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Scheduler state belongs to the command, separately from each GDK session.
#[derive(Default, Serialize, Deserialize)]
pub(super) struct Continuation {
    pub version: u32,
    pub max_sessions: Option<usize>,
    pub derived_ids: BTreeSet<String>,
    pub sessions: u32,
    pub applied: usize,
    pub costs: Costs,
    pub attempts: BTreeMap<String, u32>,
    pub parked: Vec<ParkedGoal>,
    pub parked_ids: BTreeSet<String>,
    pub known: BTreeSet<String>,
    pub resolved_changes: BTreeMap<String, Value>,
    pub cycles: BTreeMap<String, (Value, u32)>,
    pub dead_endpoint: usize,
    pub endpoint_error: String,
    pub active: Option<ActiveBatch>,
    pub completed: Vec<Receipt>,
    pub result: Option<BuildReport>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ActiveBatch {
    pub run: BatchRun,
    pub class: String,
    pub tier: Option<u8>,
    pub generation: u64,
    pub records: BTreeMap<String, Vec<String>>,
    pub report: Option<SessionReport>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Receipt {
    pub session: u32,
    pub batch: String,
    pub generation: u64,
    pub report: SessionReport,
}

pub(super) fn enabled() -> bool {
    crate::session_recovery::active() && (cfg!(target_arch = "wasm32") || crate::vfs::is_scoped())
}

pub(super) fn key(out: &Path) -> String {
    crate::session_recovery::command_key(&format!("compile:{}", out.display()))
}

impl Continuation {
    pub fn load(key: &str, max_sessions: Option<usize>) -> Result<Option<Self>, String> {
        if !enabled() {
            return Ok(None);
        }
        let Some(value) = crate::session_recovery::parent(key)? else {
            return Ok(None);
        };
        let state: Self = serde_json::from_value(value)
            .map_err(|error| format!("checkpoint-incompatible: compiler continuation: {error}"))?;
        if state.version != 1 || state.max_sessions != max_sessions {
            return Err(
                "checkpoint-incompatible: compiler continuation version or session limit changed"
                    .into(),
            );
        }
        Ok(Some(state))
    }

    pub fn fresh(max_sessions: Option<usize>) -> Self {
        Self {
            version: 1,
            max_sessions,
            ..Default::default()
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
    pub async fn save(&self, key: &str) -> Result<(), String> {
        if !enabled() {
            return Ok(());
        }
        crate::session_recovery::set_parent(
            key,
            serde_json::to_value(self)
                .map_err(|error| format!("checkpoint-failed: compiler continuation: {error}"))?,
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

impl ActiveBatch {
    /// A committed journal entry proves the batch landed even if its parent did
    /// not receive the report. Never ask the model to commit that batch again.
    pub fn committed_report(&self, out: &Path) -> Option<SessionReport> {
        let expected: BTreeSet<_> = self.run.goal_ids().into_iter().collect();
        let entries: Vec<_> = crate::goals::read_journal_range(
            out,
            self.generation,
            crate::store::read_generation(out),
        )
        .into_iter()
        .filter(|entry| {
            entry.kind == "session"
                && entry.batch.iter().cloned().collect::<BTreeSet<_>>() == expected
        })
        .collect();
        if entries.is_empty() {
            return None;
        }
        let usage = crate::session_recovery::saved_usage(&crate::session_recovery::batch_key(
            out,
            &self.run.id,
        ));
        Some(SessionReport {
            applied: entries.iter().map(|entry| entry.mutations.len()).sum(),
            rounds: usage
                .map(|v| v.2)
                .unwrap_or_else(|| entries.iter().map(|entry| entry.rounds).max().unwrap_or(0)),
            tokens: usage
                .map(|v| v.0)
                .unwrap_or_else(|| entries.iter().filter_map(|entry| entry.tokens).sum()),
            cached: usage
                .map(|v| v.1)
                .unwrap_or_else(|| entries.iter().filter_map(|entry| entry.cached_tokens).sum()),
            resolved: entries
                .iter()
                .flat_map(|entry| entry.resolved_goals.iter().map(|goal| goal.goal.clone()))
                .collect(),
            failed: None,
            stopped: None,
        })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::{model::Goal, runner::SessionRunner, session::Trace, store::Store};
    use std::cell::Cell;

    struct Script {
        calls: Cell<usize>,
        stop: Option<String>,
    }
    impl SessionRunner for Script {
        fn agent_name(&self) -> String {
            "continuation-test".into()
        }
        fn set_build_token(&self, _: Option<String>) {}
        fn run_item(&self, batch: &BatchRun, _: &Trace) -> SessionReport {
            self.calls.set(self.calls.get() + 1);
            assert_eq!(batch.id, "saved-batch");
            assert_eq!(batch.goals[0].change, serde_json::json!({"saved": true}));
            SessionReport {
                applied: 4,
                rounds: 2,
                tokens: 17,
                cached: 5,
                failed: self.stop.clone(),
                stopped: self.stop.clone(),
                resolved: batch.goal_ids(),
            }
        }
        fn ask(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String, String> {
            unreachable!()
        }
        fn ask_traced(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: Option<&Trace>,
        ) -> Result<String, String> {
            unreachable!()
        }
    }

    fn active() -> ActiveBatch {
        ActiveBatch {
            run: BatchRun {
                id: "saved-batch".into(),
                executor: Some("saved-executor".into()),
                goals: vec![Goal {
                    id: "g:review-entity:ent:gone".into(),
                    kind: "review-entity".into(),
                    class: "compile".into(),
                    target: "ent:gone".into(),
                    change: serde_json::json!({"saved": true}),
                    ..Default::default()
                }],
            },
            class: "compile".into(),
            tier: Some(2),
            generation: 0,
            records: BTreeMap::new(),
            report: None,
        }
    }

    fn state() -> Continuation {
        let mut state = Continuation::fresh(Some(9));
        state.sessions = 3;
        state.applied = 2;
        state.costs.charge("reconcile-section", "compile", 23);
        state.derived_ids.insert("g:review-entity:ent:gone".into());
        state.active = Some(active());
        state
    }

    #[test]
    fn resumes_exact_active_batch_and_acknowledges_accounting_once() {
        let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
        let _scope = crate::session_recovery::Scope::enter(None);
        let out = Path::new("/project/.jazyk/main");
        let mut store = Store::open_for_build(out);
        let project = crate::board::tests::project_for(&store);
        let control = crate::board::tests::control_auto();
        let key = key(out);
        state().save(&key).unwrap();
        let runner = Script {
            calls: Cell::new(0),
            stop: None,
        };
        let trace = Trace::to_sink(
            crate::session::TraceLevel::Normal,
            std::sync::Arc::new(|_| {}),
            Default::default(),
        );
        let run = crate::reconcile::run_sessions(
            &project,
            out,
            &trace,
            Some(9),
            &runner,
            &mut store,
            &BTreeMap::new(),
            &control,
        );
        assert_eq!(runner.calls.get(), 1);
        assert_eq!((run.sessions, run.applied, run.costs.tokens), (3, 6, 40));
        assert!(run.suspend_reason.is_none());
        let saved = Continuation::load(&key, Some(9)).unwrap().unwrap();
        assert!(saved.active.is_none());
        assert_eq!(saved.completed.len(), 1);
        // A second resume has no active work and does not charge the receipt twice.
        let again = crate::reconcile::run_sessions(
            &project,
            out,
            &trace,
            Some(9),
            &runner,
            &mut store,
            &BTreeMap::new(),
            &control,
        );
        assert_eq!(runner.calls.get(), 1);
        assert_eq!(
            (again.sessions, again.applied, again.costs.tokens),
            (3, 6, 40)
        );
    }

    #[test]
    fn journal_commit_recovers_missing_parent_report_without_worker_replay() {
        let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
        let _scope = crate::session_recovery::Scope::enter(None);
        let out = Path::new("/project/.jazyk/main");
        let mut store = Store::open_for_build(out);
        let state = state();
        state.save(&key(out)).unwrap();
        let mut commit = crate::store::Commit::session(
            state.active.as_ref().unwrap().run.goal_ids(),
            2,
            Some(17),
        );
        commit.resolved = state
            .active
            .as_ref()
            .unwrap()
            .run
            .goal_ids()
            .into_iter()
            .map(|goal| crate::model::Resolved {
                goal,
                justification: "committed before the parent acknowledgement".into(),
                evidence: Value::Null,
            })
            .collect();
        store.apply(Vec::new(), &commit);
        let project = crate::board::tests::project_for(&store);
        let control = crate::board::tests::control_auto();
        let runner = Script {
            calls: Cell::new(0),
            stop: None,
        };
        let trace = Trace::to_sink(
            crate::session::TraceLevel::Normal,
            std::sync::Arc::new(|_| {}),
            Default::default(),
        );
        let run = crate::reconcile::run_sessions(
            &project,
            out,
            &trace,
            Some(9),
            &runner,
            &mut store,
            &BTreeMap::new(),
            &control,
        );
        assert_eq!(runner.calls.get(), 0);
        assert_eq!((run.sessions, run.applied, run.costs.tokens), (3, 2, 40));
        assert!(Continuation::load(&key(out), Some(8))
            .err()
            .unwrap()
            .starts_with("checkpoint-incompatible:"));
    }

    #[test]
    fn interrupted_batch_keeps_intention_without_retry_or_parking() {
        let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
        let _scope = crate::session_recovery::Scope::enter(None);
        let out = Path::new("/project/.jazyk/main");
        let mut store = Store::open_for_build(out);
        state().save(&key(out)).unwrap();
        let project = crate::board::tests::project_for(&store);
        let control = crate::board::tests::control_auto();
        let reason = "checkpoint-failed: injected storage outage".to_string();
        let runner = Script {
            calls: Cell::new(0),
            stop: Some(reason.clone()),
        };
        let trace = Trace::to_sink(
            crate::session::TraceLevel::Normal,
            std::sync::Arc::new(|_| {}),
            Default::default(),
        );
        let run = crate::reconcile::run_sessions(
            &project,
            out,
            &trace,
            Some(9),
            &runner,
            &mut store,
            &BTreeMap::new(),
            &control,
        );
        assert_eq!(run.suspend_reason, Some(reason));
        assert!(run.parked.is_empty());
        assert_eq!(run.costs.tokens, 23);
        let saved = Continuation::load(&key(out), Some(9)).unwrap().unwrap();
        assert!(saved.active.unwrap().report.is_none());
        assert!(saved.completed.is_empty());
    }
}
