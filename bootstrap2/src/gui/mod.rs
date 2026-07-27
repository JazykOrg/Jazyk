// The GUI frontend: one local process serving the web app, the JSON API, the event
// stream, and the language server over WebSocket. Mirrors docs2/frontends/gui.md.
mod api;
mod assets;
mod diff;
mod docs;
mod events;
mod jobs;
mod lsp_ws;
mod server;
mod state;

use state::SharedState;

// Watch-mode hook: with the compile toggle on, a document change enqueues a build.
fn jobs_hook_on_docs_changed(st: &SharedState) {
    if st.watch.load(std::sync::atomic::Ordering::Relaxed) {
        st.jobs.submit(st, jobs::JobKind::Compile);
    }
}

// Watch-mode retry: a compile that ends incomplete (work parked, e.g. a transient
// endpoint outage) retries with backoff, the same loop `jazyk watch` runs. A document
// change resets the backoff by queueing a fresh compile through the hook above.
fn jobs_hook_on_job_finished(st: &SharedState, kind: &jobs::JobKind) {
    if !matches!(kind, jobs::JobKind::Compile) || !st.watch.load(std::sync::atomic::Ordering::Relaxed) {
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
        if st.watch.load(std::sync::atomic::Ordering::Relaxed) && st.jobs.running_job().is_none() {
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
}

pub fn run(proj: Project, llm: Llm, out: PathBuf, gopts: GuiOptions) -> i32 {
    let gs = GenSettings::resolve(&proj, &out);
    let dist_dir = gopts
        .gui_dist
        .clone()
        .or_else(|| std::env::var("JAZYK_GUI_DIST").ok())
        .map(PathBuf::from);
    let token = if gopts.no_token { None } else { Some(state::mint_token()) };
    let st = Arc::new(AppState {
        proj,
        llm,
        out,
        gs,
        token,
        dist_dir,
        shutdown: tokio::sync::Notify::new(),
        events: events::EventHub::new(),
        last_pending: std::sync::Mutex::new(serde_json::Value::Null),
        jobs: jobs::JobManager::new(),
        watch: std::sync::atomic::AtomicBool::new(gopts.watch),
        backoff: std::sync::atomic::AtomicU64::new(30),
    });
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
        let url = match &st.token {
            Some(t) => format!("http://127.0.0.1:{}/#token={}", addr.port(), t),
            None => format!("http://127.0.0.1:{}/", addr.port()),
        };
        println!("jazyk: gui — serving {} at {}", st.proj.root.display(), url);
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
        // Let a cancelled job reach a boundary and release the store lock before exit.
        st.jobs.cancel_running();
        st.jobs.stop();
        let _ = tokio::task::spawn_blocking(move || {
            let _ = worker.join();
        })
        .await;
        code
    })
}
