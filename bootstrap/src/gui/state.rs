// Shared state for the GUI server. One instance per `jazyk gui` process.
use crate::gen::GenSettings;
use crate::llm::Llm;
use crate::project::Project;
use std::path::PathBuf;
use std::sync::Arc;

pub struct AppState {
    // The project root is fixed for the process; the settings behind it can be
    // rewritten from the GUI and reload live (see gui/mod.rs reload_project).
    pub root: PathBuf,
    pub proj: std::sync::RwLock<Project>,
    pub llm: std::sync::RwLock<Llm>,
    pub out: PathBuf,
    pub gs: std::sync::RwLock<GenSettings>,
    // The original invocation, so a settings reload re-resolves the LLM with the
    // same flag and env precedence.
    pub cli_opts: crate::cli::Options,
    // None when --no-token: every request passes the auth check.
    pub token: Option<String>,
    // The page's origin (`https://jazyk.dev`, or the origin of --gui-url /
    // JAZYK_GUI_URL): the one origin the CORS layer admits
    // (docs/frontends/gui.md#serving).
    pub gui_origin: String,
    // --dev: the app starts with the Developer toggle on (docs/frontends/gui.md#developer-toggle).
    pub dev: bool,
    // Signals the server to stop (ctrl-c or POST /api/shutdown).
    pub shutdown: Arc<tokio::sync::Notify>,
    pub events: super::events::EventHub,
    // Board derivations coalesced per set (docs/frontends/gui.md#events).
    pub board: Arc<super::events::BoardCoalescer>,
    // Last emitted pending.changed payload per set, to emit only on movement.
    // Shared by every scoped state, since each job and watcher runs against a
    // fresh one.
    pub last_pending: Arc<std::sync::Mutex<std::collections::BTreeMap<String, serde_json::Value>>>,
    pub jobs: Arc<super::jobs::JobManager>,
    // The chat pane's sessions and their shared agent host (docs/frontends/gui.md#chat).
    pub chat: Arc<super::chat::ChatManager>,
    // Watch-mode retry backoff in seconds per set (30 doubling to 300, reset on
    // success), shared the same way (docs/frontends/gui.md#jobs).
    pub backoff: Arc<std::sync::Mutex<std::collections::BTreeMap<String, u64>>>,
}

pub type SharedState = Arc<AppState>;

// Cheap snapshots: settings are small, and a clone per request keeps every reader
// consistent without holding a lock across IO.
impl AppState {
    pub fn scoped(&self, id: Option<&str>) -> Result<SharedState, (u16, String)> {
        let workspace = crate::document_sets::Workspace::load(&self.root)?;
        self.scoped_in(&workspace, id)
    }

    // The scoped state against a workspace the caller already loaded. Sync disk reads
    // (the global LLM config) live here: call it off the async runtime.
    pub fn scoped_in(
        &self,
        workspace: &crate::document_sets::Workspace,
        id: Option<&str>,
    ) -> Result<SharedState, (u16, String)> {
        let proj = match id {
            Some(id) => workspace.resolve(Some(id))?,
            None => workspace.files_project(),
        };
        let llm = crate::cli::resolve_llm(
            &self.cli_opts,
            &proj.llm,
            &crate::project::load_global_llm(),
            |n| std::env::var(n).ok(),
        );
        Ok(Arc::new(Self {
            root: self.root.clone(),
            out: if id.is_some() {
                proj.out.clone()
            } else {
                self.root.join(".jazyk")
            },
            gs: std::sync::RwLock::new(GenSettings::resolve(&proj)),
            events: self.events.scoped(proj.set_id.clone()),
            proj: std::sync::RwLock::new(proj),
            llm: std::sync::RwLock::new(llm),
            cli_opts: self.cli_opts.clone(),
            token: self.token.clone(),
            gui_origin: self.gui_origin.clone(),
            dev: self.dev,
            shutdown: self.shutdown.clone(),
            jobs: self.jobs.clone(),
            chat: self.chat.clone(),
            board: self.board.clone(),
            last_pending: self.last_pending.clone(),
            backoff: self.backoff.clone(),
        }))
    }

    // The key of this state's set in the shared per-set maps.
    pub fn set_key(&self) -> String {
        self.proj.read().unwrap().set_id.clone().unwrap_or_default()
    }

    pub fn proj(&self) -> Project {
        self.proj.read().unwrap().clone()
    }
    pub fn llm(&self) -> Llm {
        self.llm.read().unwrap().clone()
    }
    pub fn gs(&self) -> GenSettings {
        self.gs.read().unwrap().clone()
    }
    // The workflow modes are control-plane state, not process memory: shared with
    // every worker, surviving restarts. Mirrors docs/frontends/gui.md#workflow-modes.
    pub fn control(&self) -> crate::control::Control {
        crate::control::Control::load(&self.proj(), &self.out)
    }
}

// A random hex token from the OS entropy source. Without entropy there is no token
// worth trusting, so the server does not start (no time-derived fallback).
pub fn mint_token() -> Result<String, String> {
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf).map_err(|e| format!("cannot mint a session token: {}", e))?;
    Ok(buf.iter().map(|b| format!("{:02x}", b)).collect())
}

// Whether a presented token is the expected one, in time that does not depend on
// where they differ.
pub fn token_matches(presented: Option<&str>, expected: &str) -> bool {
    use subtle::ConstantTimeEq;
    presented.is_some_and(|p| bool::from(p.as_bytes().ct_eq(expected.as_bytes())))
}

#[cfg(test)]
mod tests {
    #[test]
    fn tokens_are_random_hex_and_compare_exactly() {
        let a = super::mint_token().unwrap();
        let b = super::mint_token().unwrap();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert!(super::token_matches(Some(&a), &a));
        assert!(!super::token_matches(Some(&b), &a));
        assert!(!super::token_matches(Some(&a[..31]), &a));
        assert!(!super::token_matches(None, &a));
    }
}
