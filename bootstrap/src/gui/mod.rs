// The GUI frontend: one local process serving the web app, the JSON API, the event
// stream, and the language server over WebSocket. Mirrors docs/frontends/gui.md.
mod api;
mod assets;
mod deliverable;
mod diff;
mod docs;
mod events;
mod jobs;
mod lsp_ws;
mod server;
mod state;

use state::SharedState;

fn watch_mode(st: &SharedState) -> String {
    st.watch_mode.lock().unwrap().clone()
}

// Watch-mode hook: in `watch` mode a document change enqueues a build. The other
// modes only surface the change (docs.changed already fired).
fn jobs_hook_on_docs_changed(st: &SharedState) {
    if watch_mode(st) == "watch" {
        st.jobs.submit(st, jobs::JobKind::Compile);
    }
}

// Watch-mode retry: a compile that ends incomplete (work parked, e.g. a transient
// endpoint outage) retries with backoff, the same loop `jazyk watch` runs. A document
// change resets the backoff by queueing a fresh compile through the hook above.
fn jobs_hook_on_job_finished(st: &SharedState, kind: &jobs::JobKind) {
    if !matches!(kind, jobs::JobKind::Compile) {
        return;
    }
    // Auto generation: a finished compile with a non-empty worklist queues a gen job
    // behind it. Only compile jobs trigger it, so the chain never loops.
    // Mirrors docs/frontends/gui.md#generation.
    if st.gen_mode.lock().unwrap().as_str() == "auto" {
        let store = crate::store::Store::load(&st.out);
        if !crate::gen::pending(&store, &st.gs()).is_empty() {
            st.jobs.submit(st, jobs::JobKind::Gen { entities: vec![], force: false });
        }
    }
    if watch_mode(st) != "watch" {
        return;
    }
    let verdict = crate::store::Store::load(&st.out).status.verdict.clone();
    if verdict != "incomplete" {
        st.backoff.store(30, std::sync::atomic::Ordering::Relaxed);
        return;
    }
    let secs = st.backoff.load(std::sync::atomic::Ordering::Relaxed);
    st.backoff.store((secs * 2).min(300), std::sync::atomic::Ordering::Relaxed);
    let st = st.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(secs));
        if watch_mode(&st) == "watch" && st.jobs.running_job().is_none() {
            st.jobs.submit(&st, jobs::JobKind::Compile);
        }
    });
}

use crate::gen::GenSettings;
use crate::llm::Llm;
use crate::project::Project;
use state::AppState;
use std::path::PathBuf;
use std::sync::Arc;

pub struct GuiOptions {
    pub port: Option<u16>,
    pub no_open: bool,
    pub watch: bool,
    pub gui_dist: Option<String>,
    pub no_token: bool,
    // The original invocation, kept so a settings reload re-resolves the LLM with
    // the same flag and env precedence.
    pub cli_opts: crate::cli::Options,
}

// Re-read jazyk.toml and apply it to the running server: the project, the resolved
// LLM, and the generation settings swap in place; every reader takes fresh
// snapshots. Called after PUT /api/settings.
pub(crate) fn reload_project(st: &SharedState) {
    let mut proj = Project::load(&st.root);
    proj.out = st.out.clone();
    let global = crate::project::load_global_llm();
    let llm = crate::cli::resolve_llm(&st.cli_opts, &proj.llm, &global, |n| std::env::var(n).ok());
    *st.gs.write().unwrap() = GenSettings::resolve(&proj);
    *st.llm.write().unwrap() = llm;
    *st.proj.write().unwrap() = proj;
}

// Ask whoever holds the default port to identify itself (the unauthenticated ping).
fn probe_occupant(port: u16, our_root: &std::path::Path) -> String {
    let resp = ureq::get(&format!("http://127.0.0.1:{}/api/ping", port))
        .timeout(std::time::Duration::from_millis(400))
        .call();
    match resp.ok().and_then(|r| r.into_string().ok()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
        Some(v) if v["app"] == "jazyk-gui" => {
            let root = v["root"].as_str().unwrap_or("?");
            if root == our_root.to_string_lossy() {
                "another jazyk gui already serves this project there".to_string()
            } else {
                format!("a jazyk gui serving {}", root)
            }
        }
        _ => "another process".to_string(),
    }
}

pub fn run(proj: Project, llm: Llm, out: PathBuf, gopts: GuiOptions) -> i32 {
    let gs = GenSettings::resolve(&proj);
    let dist_dir = gopts
        .gui_dist
        .clone()
        .or_else(|| std::env::var("JAZYK_GUI_DIST").ok())
        .map(PathBuf::from);
    let token = if gopts.no_token { None } else { Some(state::mint_token()) };
    let st = Arc::new(AppState {
        root: proj.root.clone(),
        proj: std::sync::RwLock::new(proj),
        llm: std::sync::RwLock::new(llm),
        out,
        gs: std::sync::RwLock::new(gs),
        cli_opts: gopts.cli_opts.clone(),
        token,
        dist_dir,
        shutdown: tokio::sync::Notify::new(),
        events: events::EventHub::new(),
        last_pending: std::sync::Mutex::new(serde_json::Value::Null),
        jobs: jobs::JobManager::new(),
        watch_mode: std::sync::Mutex::new(if gopts.watch { "watch" } else { "queue" }.to_string()),
        gen_mode: std::sync::Mutex::new("manual".to_string()),
        backoff: std::sync::atomic::AtomicU64::new(30),
    });
    jobs::sweep_traces(&st.out);
    let worker = jobs::spawn_worker(st.clone());

    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("jazyk: gui: cannot start runtime: {}", e);
            return 1;
        }
    };
    rt.block_on(async move {
        let listener = match server::bind(gopts.port).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("jazyk: gui: {}", e);
                return 1;
            }
        };
        events::spawn_store_watcher(st.clone());
        events::spawn_docs_watcher(st.clone());
        let addr = listener.local_addr().expect("listener addr");
        // Fell back off the busy default port: say who owns it, so a stale tab or
        // bookmark on the default port is explainable at a glance.
        if gopts.port.is_none() && addr.port() != server::DEFAULT_PORT {
            let root = st.proj().root.clone();
            let occupant = tokio::task::spawn_blocking(move || probe_occupant(server::DEFAULT_PORT, &root))
                .await
                .unwrap_or_default();
            eprintln!("jazyk: gui: port {} is busy: {}", server::DEFAULT_PORT, occupant);
        }
        let url = match &st.token {
            Some(t) => format!("http://127.0.0.1:{}/#token={}", addr.port(), t),
            None => format!("http://127.0.0.1:{}/", addr.port()),
        };
        println!("jazyk: gui — serving {} at {}", st.proj().root.display(), url);
        if !gopts.no_open {
            if let Err(e) = open::that_detached(&url) {
                eprintln!("jazyk: gui: cannot open browser: {} (open {} yourself)", e, url);
            }
        }
        let code = match server::serve(listener, st.clone()).await {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("jazyk: gui: {}", e);
                1
            }
        };
        // Let a cancelled job reach a boundary and release the store lock before exit,
        // but never hang shutdown on a slow LLM call: the lock is only held during the
        // brief commit, so a bounded wait covers the risky window.
        st.jobs.cancel_running();
        st.jobs.stop();
        let join = tokio::task::spawn_blocking(move || {
            let _ = worker.join();
        });
        if tokio::time::timeout(std::time::Duration::from_secs(10), join).await.is_err() {
            eprintln!("jazyk: gui: a job is still finishing its current step; exiting anyway");
        }
        code
    })
}
