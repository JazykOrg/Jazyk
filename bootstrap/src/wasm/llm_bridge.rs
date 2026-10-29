// The bridge to the model in the page. The reconciler is synchronous all the way
// down: a session calls the model and waits for the reply before it decides what to
// stage. A browser's model APIs are all promises, and a promise cannot be awaited
// from synchronous Rust, so the two are joined the only way that composes: the
// backend runs in a worker, and a call blocks that worker on `Atomics.wait` until
// the page writes the reply into the shared buffer and wakes it.
//
// See docs/frontends/wasm.md#the-model for the protocol and what happens without
// cross-origin isolation.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/wasm/bridge.js")]
extern "C" {
    // Hands one OpenAI-shaped request body to the page and blocks until the reply
    // comes back. Returns the response body, or a message naming why there is none.
    #[wasm_bindgen(js_name = chatSync, catch)]
    fn chat_sync(body: &str) -> Result<String, JsValue>;

    // Whether the page cancelled the running chat turn, and the reset at its start.
    // See docs/frontends/wasm.md#chat.
    #[wasm_bindgen(js_name = cancelRequested)]
    fn cancel_requested_js() -> bool;
    #[wasm_bindgen(js_name = clearCancel)]
    fn clear_cancel_js();
    #[wasm_bindgen(js_name = pollInputs, catch)]
    fn poll_inputs_js() -> Result<String, JsValue>;
    #[wasm_bindgen(js_name = closeInputs)]
    fn close_inputs_js() -> bool;
}

pub fn cancel_requested() -> bool {
    cancel_requested_js()
}

pub fn clear_cancel() {
    clear_cancel_js()
}

pub fn chat(body: &str) -> Result<String, String> {
    chat_sync(body).map_err(|e| {
        // What comes back is a JS Error, whose text is on `message`; `as_string`
        // answers None for it and would throw away the only thing worth reading.
        if let Some(s) = e.as_string() {
            return s;
        }
        js_sys::Reflect::get(&e, &JsValue::from_str("message"))
            .ok()
            .and_then(|m| m.as_string())
            .unwrap_or_else(|| "the page did not answer the model call".to_string())
    })
}

pub fn take_inputs(session: &str) -> Vec<serde_json::Value> {
    let values = poll_inputs_js().ok()
        .and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).ok())
        .unwrap_or_default();
    values.into_iter().filter(|v| v["sessionId"].as_str() == Some(session)).collect()
}
pub fn close_inputs(_session: &str) -> bool { close_inputs_js() }

pub fn chat_deltas(body: &str, _delta: &mut dyn FnMut(&str, &str)) -> Result<String,String> {
    // The page may stream directly into its event reducer; model history still
    // waits for the single complete response through the synchronous bridge.
    chat(body)
}
