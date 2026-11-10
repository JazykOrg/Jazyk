// The benchmark frontend: decides whether a model is capable of driving compilation
// turns. Runs every case through the native-tool GDK runtime in a sandbox store, grades with
// deterministic checks, no LLM judge. Mirrors docs/benchmark/benchmark.md.
//
// The case definitions ARE the documentation: the fenced yaml blocks in
// docs/benchmark/cases/*.md are embedded at compile time, one source of truth.

mod cases;
mod checks;
#[cfg(test)]
mod checks_tests;
mod results;
mod run;
mod sandbox;
mod session;
#[cfg(test)]
mod tests;

pub use cases::*;
pub use checks::*;
pub use results::*;
pub use run::*;
pub use sandbox::*;
pub use session::*;
