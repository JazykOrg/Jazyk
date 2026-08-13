// The ACP bridge: jazyk as a client of one downstream ACP agent (worker and chat
// sessions), and as an agent/proxy toward IDEs. Mirrors docs/frontends/acp.md.
pub mod agent;
pub mod config;
pub mod host;
pub mod policy;
pub mod runner;
pub mod translate;
