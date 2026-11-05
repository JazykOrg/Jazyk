//! Runtime context shared by the browser and the isolated native workspace.
#[cfg(not(target_arch = "wasm32"))]
pub use crate::hosted::{emit_event, llm, model_name, root, trace};
#[cfg(target_arch = "wasm32")]
pub use crate::wasm::llm_bridge::{cancel_requested, clear_cancel};
#[cfg(target_arch = "wasm32")]
pub use crate::wasm::{emit_event, llm, model_name, root, trace};
#[cfg(not(target_arch = "wasm32"))]
pub fn clear_cancel() {}
#[cfg(not(target_arch = "wasm32"))]
pub fn cancel_requested() -> bool {
    crate::hosted::cancelled()
}

#[cfg(not(target_arch = "wasm32"))]
pub use crate::hosted::{take_inputs, close_inputs};
#[cfg(target_arch = "wasm32")]
pub use crate::wasm::llm_bridge::{take_inputs, close_inputs};

// The host's global agent context; a browser tab has none.
#[cfg(not(target_arch = "wasm32"))]
pub use crate::hosted::host_context;
#[cfg(target_arch = "wasm32")]
pub fn host_context() -> Option<std::sync::Arc<crate::agent_context::HostContext>> {
    None
}
