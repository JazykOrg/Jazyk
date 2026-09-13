// The crate root. `jazyk` is a library with two front doors: the native binary
// (src/main.rs, the CLI launcher) and the WebAssembly backend (src/wasm/, the
// same JSON API served in a browser tab). See docs/frontends/wasm.md.
//
// Modules split three ways. The core reconciler compiles everywhere. The native
// modules need a process, sockets, or threads and are gated off the wasm build.
// The wasm module exists only there.

// Core: the graph, the reconciler, and everything they need. Compiles for both
// targets against crate::vfs instead of std::fs.
pub mod align;
pub mod board;
pub mod card;
pub mod cli;
pub mod context;
pub mod control;
pub mod derive;
pub mod goals;
pub mod limits;
pub mod llm;
pub mod md;
pub mod model;
pub mod project;
pub mod reconcile;
pub mod render;
pub mod session;
pub mod store;
pub mod tools;
pub mod vfs;

// Native only: process spawning, sockets, file watching, terminal IO.
#[cfg(not(target_arch = "wasm32"))]
pub mod acp;
#[cfg(not(target_arch = "wasm32"))]
pub mod answer;
#[cfg(not(target_arch = "wasm32"))]
pub mod benchmark;
#[cfg(not(target_arch = "wasm32"))]
pub mod bind;
#[cfg(not(target_arch = "wasm32"))]
pub mod decompile;
#[cfg(not(target_arch = "wasm32"))]
pub mod docsgen;
#[cfg(not(target_arch = "wasm32"))]
pub mod feedback;
#[cfg(not(target_arch = "wasm32"))]
pub mod gen;
#[cfg(not(target_arch = "wasm32"))]
pub mod gui;
#[cfg(not(target_arch = "wasm32"))]
pub mod jsonrpc;
#[cfg(not(target_arch = "wasm32"))]
pub mod launcher;
#[cfg(not(target_arch = "wasm32"))]
pub mod lsp;
#[cfg(not(target_arch = "wasm32"))]
pub mod mcp;
#[cfg(not(target_arch = "wasm32"))]
pub mod parallel;
#[cfg(not(target_arch = "wasm32"))]
pub mod verify;
#[cfg(not(target_arch = "wasm32"))]
pub mod viewer;

// The browser backend: the API dispatch, the seeded filesystem, the WebLLM bridge.
#[cfg(target_arch = "wasm32")]
pub mod wasm;
