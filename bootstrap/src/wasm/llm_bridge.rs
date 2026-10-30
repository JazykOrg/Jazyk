// Promise-based model transport. The worker event loop remains available for
// model replies, cancellation and accepted input while the engine awaits.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/wasm/bridge.js")]
extern "C" {
    #[wasm_bindgen(js_name = chatAsync, catch)]
    async fn chat_async_js(body: &str) -> Result<JsValue, JsValue>;

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

pub async fn chat_async(body: &str) -> Result<String, String> {
    let reply = chat_async_js(body).await.map_err(error_text)?;
    reply.as_string().ok_or_else(|| "The page returned an invalid model response.".into())
}

fn error_text(error: JsValue) -> String {
    error.as_string().or_else(|| js_sys::Reflect::get(&error, &JsValue::from_str("message")).ok().and_then(|v| v.as_string()))
        .unwrap_or_else(|| "The page did not answer the model call.".into())
}

pub fn take_inputs(session: &str) -> Vec<serde_json::Value> {
    let values = poll_inputs_js().ok()
        .and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).ok())
        .unwrap_or_default();
    values.into_iter().filter(|v| v["sessionId"].as_str() == Some(session)).collect()
}
pub fn close_inputs(_session: &str) -> bool { close_inputs_js() }
