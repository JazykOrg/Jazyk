// Per-node before/after between two generations, reconstructed by replaying the
// journal forward. Journal updates store new values only, so the state at a
// generation is the fold of every committed op up to it. Approximate where the
// journal is (merges land as a remove of the absorbed id), exact for creates,
// updates, and deletes. Mirrors docs/frontends/gui.md#api and
// docs/consumers/pm.md#release-diffs-from-the-journal.
use super::state::SharedState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

// One replayed graph: node id -> body, plus the node's kind.
#[derive(Clone, Default)]
struct Replay {
    nodes: BTreeMap<String, Value>,
}

fn kind_of(id: &str) -> &'static str {
    if id.starts_with("ent:") {
        "entity"
    } else if id.starts_with("req:") {
        "requirement"
    } else if id.starts_with("view:") {
        "view"
    } else if id.starts_with("sm:") {
        "state-machine"
    } else if id.starts_with("diag:") {
        "diagnostic"
    } else {
        "node"
    }
}

impl Replay {
    fn apply(&mut self, m: &Value) {
        let op = m["op"].as_str().unwrap_or("");
        let id = m["id"].as_str().unwrap_or("").to_string();
        match op {
            "create_entity" => {
                self.nodes.insert(id, m["entity"].clone());
            }
            "create_requirement" => {
                self.nodes.insert(id, m["requirement"].clone());
            }
            "report_diagnostic" => {
                self.nodes.insert(id, m["diagnostic"].clone());
            }
            "update_entity" => {
                if let Some(Value::Object(node)) = self.nodes.get_mut(&id) {
                    for k in ["name", "definition", "stereotype", "parent", "provenance"] {
                        if !m[k].is_null() {
                            node.insert(k.into(), m[k].clone());
                        }
                    }
                    if !m["set_attributes"].is_null() {
                        node.insert("attributes".into(), m["set_attributes"].clone());
                    }
                    if let Some(aliases) = m["add_aliases"].as_array() {
                        let list = node.entry("aliases").or_insert_with(|| json!([]));
                        if let Some(a) = list.as_array_mut() {
                            for al in aliases {
                                if !a.contains(al) {
                                    a.push(al.clone());
                                }
                            }
                        }
                    }
                    if !m["add_mention"].is_null() {
                        let list = node.entry("mentions").or_insert_with(|| json!([]));
                        if let Some(a) = list.as_array_mut() {
                            a.push(m["add_mention"].clone());
                        }
                    }
                }
            }
            "update_requirement" => {
                if let Some(Value::Object(node)) = self.nodes.get_mut(&id) {
                    for k in [
                        "statement",
                        "entities",
                        "edges",
                        "transition",
                        "facets",
                        "source",
                        "provenance",
                    ] {
                        if !m[k].is_null() {
                            node.insert(k.into(), m[k].clone());
                        }
                    }
                }
            }
            "create_view" => {
                self.nodes.insert(id, m["view"].clone());
            }
            "update_view" => {
                if let Some(Value::Object(node)) = self.nodes.get_mut(&id) {
                    for k in ["title", "members", "query", "collapse"] {
                        if !m[k].is_null() {
                            node.insert(k.into(), m[k].clone());
                        }
                    }
                    // Any mutation on a default view makes it curated.
                    node.insert("default".into(), json!(false));
                }
            }
            "ratify_provenance" => {
                if let Some(Value::Object(node)) = self.nodes.get_mut(&id) {
                    node.insert("source".into(), m["source"].clone());
                    node.remove("provenance");
                }
            }
            "bump_limit" => {
                if let Some(Value::Object(node)) = self.nodes.get_mut(&id) {
                    let limits = node.entry("limits").or_insert_with(|| json!({}));
                    if let Some(o) = limits.as_object_mut() {
                        if let Some(l) = m["limit"].as_str() {
                            o.insert(l.to_string(), m["value"].clone());
                        }
                    }
                }
            }
            "resolve_diagnostic" => {
                if let Some(Value::Object(node)) = self.nodes.get_mut(&id) {
                    node.insert("lifecycle".into(), json!("resolved"));
                }
            }
            "triage_diagnostic" => {
                if let Some(Value::Object(node)) = self.nodes.get_mut(&id) {
                    node.insert("triage".into(), m["triage"].clone());
                }
            }
            "delete_entity" | "delete_requirement" | "delete_view" | "retract_decree" => {
                self.nodes.remove(&id);
            }
            "merge_entities" => {
                // The absorbed node disappears; the survivor's unions land in later
                // update ops when journaled, otherwise stay approximate.
                if let Some(absorb) = m["absorb"].as_str() {
                    self.nodes.remove(absorb);
                }
            }
            _ => {} // set_coverage and unknown ops do not shape nodes
        }
    }
}

fn replay_to(
    out: &Path,
    upto: u64,
    mut from_snapshot: Option<(u64, &mut Option<Replay>)>,
) -> Replay {
    let mut r = Replay::default();
    for g in 1..=upto {
        if let Some((at, slot)) = &mut from_snapshot {
            if g == *at + 1 && slot.is_none() {
                **slot = Some(r.clone());
            }
        }
        let f = out.join("journal").join(format!("g{}.yaml", g));
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let Ok(entry) = serde_norway::from_str::<Value>(&text) else {
            continue;
        };
        for m in entry["mutations"].as_array().cloned().unwrap_or_default() {
            r.apply(&m);
        }
    }
    if let Some((at, slot)) = from_snapshot {
        if slot.is_none() && at >= upto {
            *slot = Some(r.clone());
        }
    }
    r
}

#[derive(Deserialize)]
pub struct DiffQ {
    from: u64,
    to: u64,
}

// The range a request may ask for: `from` at least 1, `to` no further than the
// store's generation. Replay walks one journal file per generation, so an
// unclamped `to` would spin through billions of missing files.
pub(crate) fn clamp_range(from: u64, to: u64, generation: u64) -> Result<(u64, u64), String> {
    if from > to {
        return Err("from must be <= to".into());
    }
    if from == 0 {
        return Err("from must be at least 1".into());
    }
    Ok((from, to.min(generation)))
}

pub async fn diff(State(st): State<SharedState>, Query(p): Query<DiffQ>) -> Response {
    let generation = crate::store::read_generation(&st.out);
    let (from, to) = match clamp_range(p.from, p.to, generation) {
        Ok(r) => r,
        Err(e) => return super::api::err(StatusCode::BAD_REQUEST, e),
    };
    let out = st.out.clone();
    let result = tokio::task::spawn_blocking(move || {
        let p = DiffQ { from, to };
        let mut before_slot: Option<Replay> = None;
        let after = replay_to(&out, p.to, Some((p.from, &mut before_slot)));
        let before = before_slot.unwrap_or_default();
        let mut changes: Map<String, Value> = Map::new();
        let ids: std::collections::BTreeSet<&String> =
            before.nodes.keys().chain(after.nodes.keys()).collect();
        for id in ids {
            let b = before.nodes.get(id);
            let a = after.nodes.get(id);
            let change = match (b, a) {
                (None, Some(a)) => json!({ "kind": kind_of(id), "change": "added", "after": a }),
                (Some(b), None) => json!({ "kind": kind_of(id), "change": "removed", "before": b }),
                (Some(b), Some(a)) if b != a => {
                    json!({ "kind": kind_of(id), "change": "changed", "before": b, "after": a })
                }
                _ => continue,
            };
            changes.insert(id.clone(), change);
        }
        json!({ "from": p.from, "to": p.to, "changes": changes })
    })
    .await
    .expect("diff task panicked");
    Json(result).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    // The range is clamped to what the journal can hold: `to` past the store's
    // generation folds back to it, `from` below 1 and an inverted range are refused.
    #[test]
    fn range_clamps_to_the_generation_and_refuses_nonsense() {
        assert_eq!(clamp_range(1, 3, 10), Ok((1, 3)));
        assert_eq!(clamp_range(2, 99_999_999, 10), Ok((2, 10)));
        assert!(clamp_range(5, 2, 10).is_err());
        assert!(clamp_range(0, 2, 10).is_err());
    }

    // Replaying creates, updates, and deletes reconstructs a node's before and
    // after; a bump lands in the node's limits.
    #[test]
    fn replay_folds_ops_in_order() {
        let mut r = Replay::default();
        r.apply(&json!({"op": "create_entity", "id": "ent:a", "entity": {"name": "A"}}));
        r.apply(&json!({"op": "update_entity", "id": "ent:a", "definition": "an a"}));
        r.apply(&json!({"op": "bump_limit", "id": "ent:a", "limit": "requirements-per-entity", "value": 90}));
        assert_eq!(r.nodes["ent:a"]["name"], "A");
        assert_eq!(r.nodes["ent:a"]["definition"], "an a");
        assert_eq!(r.nodes["ent:a"]["limits"]["requirements-per-entity"], 90);
        r.apply(&json!({"op": "delete_entity", "id": "ent:a"}));
        assert!(!r.nodes.contains_key("ent:a"));
        assert_eq!(kind_of("req:x"), "requirement");
        assert_eq!(kind_of("zzz"), "node");
    }
}
