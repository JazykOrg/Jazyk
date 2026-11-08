// The API's values, computed once and served by two transports: the axum handlers
// of `jazyk gui` on a machine, and the dispatch of the browser backend in a tab.
// A handler in either transport is a thin wrapper around a function here.
// See docs/frontends/wasm.md#one-api-two-transports.
pub mod binary_documents;
pub mod board;
pub mod card;
pub mod deliverable;
pub mod docs;
pub mod facts;
pub mod find;
pub mod inbox;
pub mod outfiles;
pub mod overview;
pub mod product;
pub mod projfiles;
pub mod status;
pub mod views;
pub mod workers;

// A read the value functions refuse: the HTTP status and the reason, which each
// transport answers in its own envelope.
pub type Refusal = (u16, String);

pub mod dispatch;
pub mod sets;
