// The API's values, computed once and served by two transports: the axum handlers
// of `jazyk gui` on a machine, and the dispatch of the browser backend in a tab.
// A handler in either transport is a thin wrapper around a function here.
// See docs/frontends/wasm.md#one-api-two-transports.
pub mod board;
pub mod card;
pub mod docs;
pub mod status;
pub mod views;
pub mod workers;

#[cfg(target_arch = "wasm32")]
pub mod dispatch;
