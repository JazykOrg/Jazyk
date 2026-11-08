// The WebAssembly backend: the same compiler the native binary runs, answering the
// same API, against an in-memory filesystem and the model in the page.
// See docs/frontends/wasm.md.
pub mod chat;
pub mod fs_bridge;
pub mod llm_bridge;
pub mod runner;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/wasm/trace.js")]
extern "C" {
    #[wasm_bindgen(js_name = emitTrace)]
    fn emit_trace(json: &str);
    #[wasm_bindgen(js_name = emitEvent)]
    fn emit_event_js(json: &str);
}

// One event to the page, in the native event stream's shape: `{type, ...}`.
// See docs/frontends/wasm.md#chat.
pub fn emit_event(kind: &str, mut body: serde_json::Value) {
    body["type"] = serde_json::json!(kind);
    emit_event_js(&body.to_string());
}

// The project the tab is serving. One per module instance, set by `boot`.
thread_local! {
    static ROOT: std::cell::RefCell<std::path::PathBuf> =
        std::cell::RefCell::new(std::path::PathBuf::from("/project"));
}

pub fn root() -> std::path::PathBuf {
    ROOT.with(|r| r.borrow().clone())
}

// A panic in a tab is a blank page unless it says something, so the first thing the
// module does is route panics to the console.
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

// Write one file into the in-memory tree. The host calls this once per file of the
// seeded project before `boot` (docs/frontends/wasm.md#filesystem).
#[wasm_bindgen]
pub fn seed_file(path: &str, bytes: &[u8]) {
    crate::vfs::seed(std::path::Path::new(path), bytes.to_vec());
}

// Empty the tree. The host calls this before seeding another project, so a second
// demo opened in the tab does not inherit the first one's files.
#[wasm_bindgen]
pub fn reset() {
    crate::vfs::reset();
}

// Adopt the seeded tree as the project. Answers the resolved project as JSON, or an
// error naming what the tree is missing.
#[wasm_bindgen]
pub fn boot(root_path: &str) -> String {
    let root = std::path::PathBuf::from(root_path);
    ROOT.with(|r| *r.borrow_mut() = root.clone());
    // A tree with no jazyk.toml boots too, with the defaults: the setup wizard
    // writes the file (docs/frontends/gui.md#the-setup-wizard).
    // Share the native worker allowance; the selected model owns its ceiling.
    crate::llm::set_max_completion_tokens(crate::limits::DEFAULT_MAX_COMPLETION_TOKENS);
    // Conversations belong to the project they were about.
    chat::reset();
    let p = crate::project::Project::load(&root);
    serde_json::json!({
        "ok": true,
        "root": p.root.to_string_lossy(),
        "out": p.out.to_string_lossy(),
        "docsGlob": p.docs_glob,
        "roots": p.roots,
        "files": crate::vfs::list_all().len(),
    })
    .to_string()
}

// Turn on the write-through: from here every mutation of the tree is also posted
// to the page, which mirrors it into the folder it opened.
#[wasm_bindgen]
pub fn enable_fs_sink() {
    fs_bridge::enable();
}

// Every file in the tree, for the host to persist or download.
#[wasm_bindgen]
pub fn list_files() -> String {
    serde_json::json!(crate::vfs::list_all()
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>())
    .to_string()
}

// One file's bytes, for the host to save the project to a folder or download it.
// Undefined when the tree has no such file.
#[wasm_bindgen]
pub fn read_file(path: &str) -> Option<Vec<u8>> {
    crate::vfs::read(path).ok()
}

// The model the page holds. The host names it at boot; the base url is the bridge,
// not an endpoint, so `Llm` carries a marker where a URL would go.
thread_local! {
    static MODEL: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
}

pub fn model_name() -> String {
    MODEL.with(|m| m.borrow().clone())
}

// The client every session uses. Its transport is the bridge to the page.
pub fn llm() -> crate::llm::Llm {
    crate::llm::Llm {
        api: "chat_completions".into(),
        reasoning_summary: None,
        base_url: "in-page".to_string(),
        model: model_name(),
        api_key: String::new(),
        temperature: Some(0.0),
        trace: None,
    }
}

// Name the model the page loaded, so traces and the project reply say which one ran.
#[wasm_bindgen]
pub fn set_model(name: &str) {
    MODEL.with(|m| *m.borrow_mut() = name.to_string());
}

// The trace of the running build. A build runs synchronously inside the worker, so
// there is no moment between events at which the page could poll: each event is
// posted to the page as the compiler emits it (docs/frontends/wasm.md#the-trace).
pub fn trace() -> crate::session::Trace {
    crate::session::Trace::to_sink(
        crate::session::TraceLevel::Normal,
        std::sync::Arc::new(|ev: &crate::session::TraceEvent| {
            if let Ok(s) = serde_json::to_string(ev) {
                emit_trace(&s);
            }
        }),
        Default::default(),
    )
}

// One request, the same shape the native server answers over HTTP: a method, a path
// under /api, and a JSON body. See docs/frontends/wasm.md#one-api-two-transports.
#[wasm_bindgen]
pub fn request(method: &str, path: &str, body: &str) -> String {
    crate::webapi::dispatch::request(method, path, body)
}

#[wasm_bindgen]
pub async fn request_async(method: String, path: String, body: String) -> String {
    let _recovery = crate::session_recovery::Scope::enter(None);
    crate::webapi::dispatch::request_async(&method, &path, &body).await
}
