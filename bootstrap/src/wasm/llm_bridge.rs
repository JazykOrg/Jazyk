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
