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
    // Set by --gui-dist / JAZYK_GUI_DIST: serve frontend assets from disk instead of
    // the embedded dist.
    pub dist_dir: Option<PathBuf>,
    // Signals the server to stop (ctrl-c or POST /api/shutdown).
    pub shutdown: tokio::sync::Notify,
    pub events: super::events::EventHub,
    // Last emitted pending.changed payload, to emit only on movement.
    pub last_pending: std::sync::Mutex<serde_json::Value>,
    pub jobs: super::jobs::JobManager,
    // The watch mode: off | queue | watch (docs/frontends/gui.md#watch). Default
    // queue: changes queue visibly, compiling stays an explicit click; the automatic
    // loop spends LLM budget, so it is opt-in.
    pub watch_mode: std::sync::Mutex<String>,
    // The generation mode: manual | auto (docs/frontends/gui.md#generation). In auto,
    // a finished compile with a non-empty worklist queues a gen job behind it.
    pub gen_mode: std::sync::Mutex<String>,
    // Watch-mode retry backoff in seconds (30 doubling to 300, reset on success).
    pub backoff: std::sync::atomic::AtomicU64,
}

pub type SharedState = Arc<AppState>;

// Cheap snapshots: settings are small, and a clone per request keeps every reader
// consistent without holding a lock across IO.
impl AppState {
    pub fn proj(&self) -> Project {
        self.proj.read().unwrap().clone()
    }
    pub fn llm(&self) -> Llm {
        self.llm.read().unwrap().clone()
    }
    pub fn gs(&self) -> GenSettings {
        self.gs.read().unwrap().clone()
    }
}

// A random hex token from the OS entropy source. No rand crate: this is the one place
// the binary needs randomness.
pub fn mint_token() -> String {
    let mut buf = [0u8; 16];
    if std::io::Read::read_exact(
        &mut std::fs::File::open("/dev/urandom").expect("open /dev/urandom"),
        &mut buf,
    )
    .is_err()
    {
        // Fallback: time-derived, still unguessable enough for a localhost session.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        buf[..16].copy_from_slice(&t.to_le_bytes());
    }
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}
