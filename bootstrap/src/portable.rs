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
