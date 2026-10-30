//! Fixed generation steps retain their exact prompts and replies. Replaying a
//! completed step only repeats deterministic local writes, never model requests.
use super::{progress, GenSettings, Ledger};
use crate::{
    runner::{BatchRun, SessionReport, SessionRunner},
    session::Trace,
    store::Store,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Serialize, Deserialize)]
struct Request {
    system: String,
    user: String,
    reply: Option<Result<String, String>>,
}

#[derive(Serialize, Deserialize)]
struct State {
    task: Value,
    ledger: Ledger,
    requests: BTreeMap<String, Request>,
    result: Option<Result<usize, String>>,
}

struct Runner<'a> {
    inner: &'a dyn SessionRunner,
    progress: RefCell<progress::Progress<State>>,
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl SessionRunner for Runner<'_> {
    fn agent_name(&self) -> String {
        self.inner.agent_name()
    }
    fn set_build_token(&self, token: Option<String>) {
        self.inner.set_build_token(token);
    }
    async fn run_item(&self, batch: &BatchRun, trace: &Trace) -> SessionReport {
        self.inner.run_item(batch, trace).await
    }
    async fn ask(
        &self,
        system: &str,
        user: &str,
        label: &str,
        step: &str,
    ) -> Result<String, String> {
        let key = format!("{label}:{step}");
        {
            let mut progress = self.progress.borrow_mut();
            if let Some(saved) = progress.data.requests.get(&key) {
                if saved.system != system || saved.user != user {
                    return Err(format!(
                        "checkpoint-incompatible: generation pipeline prompt changed at {key}"
                    ));
                }
                if let Some(reply) = &saved.reply {
                    return reply.clone();
                }
            } else {
                progress.data.requests.insert(
                    key.clone(),
                    Request {
                        system: system.into(),
                        user: user.into(),
                        reply: None,
                    },
                );
            }
        }
        self.progress.borrow().save().await?;
        let result = self.inner.ask(system, user, label, step).await;
        if result
            .as_ref()
            .err()
            .is_some_and(|reason| crate::session_recovery::is_interruption(reason))
        {
            return result;
        }
        self.progress
            .borrow_mut()
            .data
            .requests
            .get_mut(&key)
            .unwrap()
            .reply = Some(result.clone());
        self.progress.borrow().save().await?;
        result
    }
    async fn ask_traced(
        &self,
        system: &str,
        user: &str,
        label: &str,
        step: &str,
        _: Option<&Trace>,
    ) -> Result<String, String> {
        self.ask(system, user, label, step).await
    }
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub(super) async fn run(
    store: &Store,
    runner: &dyn SessionRunner,
    gs: &GenSettings,
    id: &str,
    task: &Value,
) -> Result<usize, String> {
    let signature =
        serde_json::json!({"settings":gs,"entity":id,"facts":super::fact_hash(store,id)});
    let key = progress::command_key("generation-pipeline", &store.out, &signature);
    let baseline_key = format!("{key}:baselines");
    let saved = progress::Progress::load(key, signature, || State {
        task: task.clone(),
        ledger: Ledger::load(&store.out),
        requests: BTreeMap::new(),
        result: None,
    })?;
    if let Some(result) = &saved.data.result {
        return result.clone();
    }
    saved.save().await?;
    let (task, ledger) = (saved.data.task.clone(), saved.data.ledger.clone());
    let runner = Runner {
        inner: runner,
        progress: RefCell::new(saved),
    };
    let result = super::gen_one_inner(store, &runner, gs, id, &task, &ledger, &baseline_key).await;
    if result
        .as_ref()
        .err()
        .is_some_and(|reason| crate::session_recovery::is_interruption(reason))
    {
        return result;
    }
    runner.progress.borrow_mut().data.result = Some(result.clone());
    runner.progress.borrow().save().await?;
    result
}

/// The next model/parent checkpoint atomically saves this set and the writes that
/// follow it. A replay must preserve the original diff baseline.
pub(super) fn first_baseline(key: &str, path: &str) -> Result<bool, String> {
    if !progress::enabled() {
        return Ok(true);
    }
    let mut value = crate::session_recovery::parent(key)?
        .unwrap_or_else(|| serde_json::json!({"version":1,"paths":[]}));
    if value["version"] != 1 {
        return Err("checkpoint-incompatible: generation baseline version changed".into());
    }
    let paths = value["paths"]
        .as_array_mut()
        .ok_or("checkpoint-incompatible: malformed generation baseline paths")?;
    if paths.iter().any(|item| item == path) {
        return Ok(false);
    }
    paths.push(Value::String(path.into()));
    crate::session_recovery::set_parent(key, value);
    Ok(true)
}

pub(super) fn step_error(context: String, error: String) -> String {
    if crate::session_recovery::is_interruption(&error) {
        error
    } else {
        format!("{context}: {error}")
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct Model(Cell<usize>);
    impl SessionRunner for Model {
        fn agent_name(&self) -> String {
            "step-test".into()
        }
        fn set_build_token(&self, _: Option<String>) {}
        fn run_item(&self, _: &BatchRun, _: &Trace) -> SessionReport {
            unreachable!()
        }
        fn ask(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String, String> {
            self.0.set(self.0.get() + 1);
            Ok("retained answer".into())
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

    fn runner<'a>(inner: &'a Model) -> Runner<'a> {
        Runner {
            inner,
            progress: RefCell::new(
                progress::Progress::load(
                    "pipeline-test".into(),
                    serde_json::json!({"entity":"ent:test"}),
                    || State {
                        task: Value::Null,
                        ledger: Ledger::default(),
                        requests: BTreeMap::new(),
                        result: None,
                    },
                )
                .unwrap(),
            ),
        }
    }

    #[test]
    fn saved_pipeline_step_returns_its_reply_without_repeating_inference() {
        let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
        let _scope = crate::session_recovery::Scope::enter(None);
        let model = Model(Cell::new(0));
        assert_eq!(
            runner(&model)
                .ask("system", "original prompt", "gen test", "product")
                .unwrap(),
            "retained answer"
        );
        assert_eq!(
            runner(&model)
                .ask("system", "original prompt", "gen test", "product")
                .unwrap(),
            "retained answer"
        );
        assert_eq!(model.0.get(), 1);
        assert!(runner(&model)
            .ask("system", "changed prompt", "gen test", "product")
            .unwrap_err()
            .starts_with("checkpoint-incompatible:"));
        assert_eq!(model.0.get(), 1);
    }

    #[test]
    fn replayed_file_write_preserves_the_original_baseline() {
        let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
        let _scope = crate::session_recovery::Scope::enter(None);
        let out = std::path::Path::new("/project/.jazyk/main");
        let gs = GenSettings::from_out(out);
        crate::vfs::create_dir_all(&gs.deliverable).unwrap();
        crate::vfs::write(gs.deliverable.join("product.rs"), "original").unwrap();
        super::super::snapshot_pipeline_baseline(
            out,
            &gs,
            "product.rs",
            &mut Default::default(),
            "baseline-step",
        )
        .unwrap();
        crate::vfs::write(gs.deliverable.join("product.rs"), "new content").unwrap();
        super::super::snapshot_pipeline_baseline(
            out,
            &gs,
            "product.rs",
            &mut Default::default(),
            "baseline-step",
        )
        .unwrap();
        assert_eq!(
            crate::vfs::read_to_string(out.join("deliverable-baseline/product.rs")).unwrap(),
            "original"
        );
        assert_eq!(
            step_error("manifest".into(), "checkpoint-failed: unavailable".into()),
            "checkpoint-failed: unavailable"
        );
    }
}
