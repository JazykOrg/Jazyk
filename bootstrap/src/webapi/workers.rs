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

// The workflow modes as the wire reports them. `mode` keeps the legacy select values
// (`queue` = manual, `watch` = auto) beside the control-plane names. Mirrors
// docs/compiler/control-plane.md#modes-and-releases and docs/frontends/wasm.md#workflow-modes-in-a-tab.
pub fn watch_value(c: &Control) -> Value {
    json!({
        "mode": if c.compile == "auto" { "watch" } else { "queue" },
        "compile": c.compile,
        "gen": c.generate,
        "worker": c.worker,
    })
}

// Set the workflow modes. Either field alone leaves the other untouched; they persist
// in control.yaml, where every worker reads them. `compile` takes the control-plane
// names and `mode` the legacy ones.
pub fn watch_write(proj: &Project, out: &Path, body: &Value) -> Result<Value, super::Refusal> {
    let bad = |m: &str| Err((400, m.to_string()));
    let mut c = Control::load(proj, out);
    let compile = match body["compile"].as_str() {
        Some(m) => Some(m),
        None => match body["mode"].as_str() {
            Some("watch") => Some("auto"),
            Some("queue") | Some("off") => Some("manual"),
            Some(_) => return bad("mode must be queue or watch (compile: manual or auto)"),
            None => None,
        },
    };
    if let Some(m) = compile {
        if !["manual", "auto"].contains(&m) {
            return bad("compile must be manual or auto");
        }
        c.compile = m.to_string();
    }
    if let Some(gen) = body["gen"].as_str() {
        if !["manual", "auto"].contains(&gen) {
            return bad("gen must be manual or auto");
        }
        c.generate = gen.to_string();
    } else if !body["gen"].is_null() {
        return bad("gen must be a string");
    }
    if let Some(w) = body["worker"].as_str() {
        if !["internal", "agent", "any"].contains(&w) {
            return bad("worker must be internal, agent, or any");
        }
        c.worker = w.to_string();
    }
    c.save(out);
    Ok(watch_value(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> (Project, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "jazyk-webapi-watch-{}-{}",
            name,
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&root).ok();
        crate::vfs::create_dir_all(root.join("docs")).unwrap();
        crate::vfs::write({ let path = root.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path }, "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n").unwrap();
        let proj = Project::load(&root);
        let out = proj.out.clone();
        (proj, out)
    }

    #[test]
    fn the_modes_read_their_defaults_and_persist_a_write() {
        let (proj, out) = fixture("persist");
        let v = watch_value(&Control::load(&proj, &out));
        assert_eq!(v["compile"], "manual");
        assert_eq!(v["mode"], "queue");
        assert_eq!(v["gen"], "manual");
        let v = watch_write(&proj, &out, &json!({"compile": "auto"})).unwrap();
        assert_eq!(v["compile"], "auto");
        assert_eq!(v["mode"], "watch");
        // Persisted: a fresh load reads it, and the other mode stayed.
        let c = Control::load(&proj, &out);
        assert_eq!(c.compile, "auto");
        assert_eq!(c.generate, "manual");
        // The legacy name sets it back.
        let v = watch_write(&proj, &out, &json!({"mode": "queue"})).unwrap();
        assert_eq!(v["compile"], "manual");
        let v = watch_write(&proj, &out, &json!({"gen": "auto"})).unwrap();
        assert_eq!(v["gen"], "auto");
        assert_eq!(v["compile"], "manual");
    }

    #[test]
    fn a_bad_mode_is_refused_and_writes_nothing() {
        let (proj, out) = fixture("refuse");
        for body in [
            json!({"compile": "sometimes"}),
            json!({"mode": "later"}),
            json!({"gen": 3}),
            json!({"worker": "robot"}),
        ] {
            let e = watch_write(&proj, &out, &body).unwrap_err();
            assert_eq!(e.0, 400, "{}", body);
        }
        assert!(!Control::path(&out).exists());
    }
}
