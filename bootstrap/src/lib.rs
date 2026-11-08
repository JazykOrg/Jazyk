// The crate root. `jazyk` is a library with two front doors: the native binary
// (src/main.rs, the CLI launcher) and the WebAssembly backend (src/wasm/, the
// same JSON API served in a browser tab). See docs/frontends/wasm.md.
//
// Modules split three ways. The core reconciler compiles everywhere. The native
// modules need a process, sockets, or threads and are gated off the wasm build.
// The wasm module exists only there.

// Wide argument lists and tuple-shaped locals carry the documented records through one
// call; naming each would add types used once.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

// Core: the graph, the reconciler, and everything they need. Compiles for both
// targets against crate::vfs instead of std::fs.
pub mod agent_context;
pub mod agent_workflows;
pub mod agents_md;
pub mod align;
pub mod answer;
pub mod bind;
pub mod board;
pub mod card;
pub mod context;
pub mod control;
pub mod conversation;
pub mod costs;
pub mod decompile;
pub mod derive;
pub mod diagnostic_locations;
pub mod docsgen;
pub mod document_sets;
#[cfg(all(feature = "native-documents", not(target_arch = "wasm32")))]
pub mod documents;
pub mod edit_watch;
pub mod effects;
pub mod feedback;
pub mod gen;
pub mod goals;
pub mod harness;
#[cfg(not(target_arch = "wasm32"))]
pub mod hosted;
pub mod ids;
pub mod inprocess;
pub mod input;
pub mod limits;
pub mod live_preview;
pub mod llm;
#[cfg(not(target_arch = "wasm32"))]
mod local_socket;
pub mod mcp;
pub mod md;
pub mod model;
pub mod payload;
pub mod portable;
pub mod project;
pub mod project_context;
pub mod protocol;
pub mod reconcile;
pub mod render;
pub mod runner;
mod scoped_state;
pub mod session;
pub mod session_recovery;
pub mod store;
pub mod sys;
pub mod task_runtime;
pub mod tools;
pub mod verify;
pub mod vfs;
pub mod webapi;

// Native only: process spawning, sockets, file watching, terminal IO.
#[cfg(not(target_arch = "wasm32"))]
pub mod acp;
#[cfg(not(target_arch = "wasm32"))]
pub mod benchmark;
#[cfg(not(target_arch = "wasm32"))]
pub mod cli;
#[cfg(not(target_arch = "wasm32"))]
pub mod editor_lsp;
#[cfg(not(target_arch = "wasm32"))]
pub mod gui;
#[cfg(not(target_arch = "wasm32"))]
pub mod jsonrpc;
#[cfg(not(target_arch = "wasm32"))]
pub mod launcher;
#[cfg(not(target_arch = "wasm32"))]
pub mod lsp;
#[cfg(not(target_arch = "wasm32"))]
pub mod menu;
#[cfg(not(target_arch = "wasm32"))]
pub mod replay;
#[cfg(not(target_arch = "wasm32"))]
pub mod setup;
#[cfg(not(target_arch = "wasm32"))]
pub mod start;
#[cfg(not(target_arch = "wasm32"))]
pub mod viewer;

// The browser backend: the API dispatch, the seeded filesystem, the WebLLM bridge.
#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub mod workflow;

pub mod model_api;

// Every schema in docs/ with a jazyk.org `$id` is published as its JSON form under
// site/schemas/, and the two never drift. `JAZYK_WRITE_SCHEMAS=1` writes the files.
// Mirrors docs/site.md#schemas.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod site_schemas {
    use std::path::{Path, PathBuf};

    const PREFIX: &str = "https://jazyk.org/schemas/";

    fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                sources(&p, out);
            } else if p.to_string_lossy().ends_with(".schema.yaml") {
                out.push(p);
            }
        }
    }

    #[test]
    fn published_schemas_match_their_yaml_sources() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let site = root.join("site/schemas");
        let write = std::env::var("JAZYK_WRITE_SCHEMAS").is_ok_and(|v| v == "1");
        let mut found = Vec::new();
        sources(&root.join("docs"), &mut found);
        found.sort();
        let mut published = std::collections::BTreeSet::new();
        let mut stale = Vec::new();
        for src in found {
            let yaml: serde_norway::Value =
                serde_norway::from_str(&std::fs::read_to_string(&src).unwrap())
                    .unwrap_or_else(|e| panic!("{}: {e}", src.display()));
            let Some(name) = yaml
                .get("$id")
                .and_then(|v| v.as_str())
                .and_then(|id| id.strip_prefix(PREFIX))
                .map(String::from)
            else {
                continue;
            };
            let json = format!("{}\n", serde_json::to_string_pretty(&yaml).unwrap());
            let target = site.join(&name);
            if write {
                std::fs::create_dir_all(&site).unwrap();
                std::fs::write(&target, &json).unwrap();
            } else if std::fs::read_to_string(&target).ok().as_deref() != Some(json.as_str()) {
                stale.push(format!("{} from {}", target.display(), src.display()));
            }
            published.insert(name);
        }
        for e in std::fs::read_dir(&site).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !published.contains(&name) {
                stale.push(format!(
                    "{}: no schema in docs/ names it",
                    e.path().display()
                ));
            }
        }
        assert!(
            stale.is_empty(),
            "published schemas drifted (run with JAZYK_WRITE_SCHEMAS=1):\n{}",
            stale.join("\n")
        );
    }
}
