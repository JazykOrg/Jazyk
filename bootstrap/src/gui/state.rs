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
    // Last emitted pending.changed payload, to emit only on movement.
    pub last_pending: std::sync::Mutex<serde_json::Value>,
    pub jobs: Arc<super::jobs::JobManager>,
    // The chat pane's sessions and their shared agent host (docs/frontends/gui.md#chat).
    pub chat: Arc<super::chat::ChatManager>,
    // Watch-mode retry backoff in seconds (30 doubling to 300, reset on success).
    pub backoff: std::sync::atomic::AtomicU64,
}

pub type SharedState = Arc<AppState>;

// Cheap snapshots: settings are small, and a clone per request keeps every reader
// consistent without holding a lock across IO.
impl AppState {
    pub fn scoped(&self, id: Option<&str>) -> Result<SharedState, (u16, String)> {
        let workspace = crate::document_sets::Workspace::load(&self.root)?;
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
            last_pending: std::sync::Mutex::new(serde_json::Value::Null),
            backoff: std::sync::atomic::AtomicU64::new(30),
        }))
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

// A random hex token from the OS entropy source. No rand crate: this is the one place
// the binary needs randomness.
pub fn mint_token() -> String {
    let mut buf = [0u8; 16];
    if getrandom::fill(&mut buf).is_err() {
        // Fallback: time-derived, still unguessable enough for a localhost session.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        buf[..16].copy_from_slice(&t.to_le_bytes());
    }
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}
