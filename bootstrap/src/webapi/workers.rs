// The workers strip: the workflow modes, who is attached, what they hold, what is
// gated, what is actionable, and the decompile worklist. Target-neutral
// (docs/frontends/wasm.md#one-api-two-transports).
use crate::control::Control;
use crate::gen::GenSettings;
use crate::project::Project;
use serde_json::{json, Value};
use std::path::Path;

pub fn workers_snapshot(proj: &Project, out: &Path, gs: &GenSettings, c: &Control) -> Value {
    let board = crate::board::Board::compute(proj, out);
    let graph_kinds = crate::board::Board::graph_kinds();
    let store = crate::store::Store::load(out);
    let unclaimed = crate::bind::unclaimed(proj, &store, gs);
    json!({
        "workflow": {"compile": c.compile, "generate": c.generate, "worker": c.worker},
        "workers": crate::control::workers(out),
        "leases": crate::control::leases(out).values().collect::<Vec<_>>(),
        "gated": {
            "compile": board.gated_of(&graph_kinds),
            // Bind goals gate under the generate release beside generation.
            "generate": board.gated_of(&["generate", "bind"]),
        },
        "actionable": {
            "compile": board.ready_of(&graph_kinds),
            "bind": board.ready_of(&["bind"]),
            "generate": board.ready_of(&["generate"]),
            "verify": board.ready_of(&["verify"]),
        },
        "board": board.counts(),
        // The unclaimed report: the decompile worklist.
        "unclaimed": unclaimed.len(),
        "decompileReleased": c.released.decompile,
    })
}
