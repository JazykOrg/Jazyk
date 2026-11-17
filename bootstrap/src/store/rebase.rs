// Rebase at commit: a changeset staged against an older generation meets the commits
// that landed since. Additive mutations commute; the rest conflict with what changed
// the same node, section, or goal. Mirrors docs/compiler/graph.md#commit-conflicts.
use super::*;
use serde::Serialize;
use serde_json::Value;

// One collision between a staged changeset and a commit that landed first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Conflict {
    // node | section | goal
    pub kind: String,
    // The node id, `doc#section`, or goal id.
    pub id: String,
    // The generation that changed it, and that commit's kind and batch.
    pub generation: u64,
    #[serde(rename = "byKind")]
    pub by_kind: String,
    #[serde(rename = "byBatch", skip_serializing_if = "Vec::is_empty")]
    pub by_batch: Vec<String>,
    // What the winner did: the mutation's op, or the goal's justification.
    pub detail: String,
    // The staged mutation's index (0-based) a node or section conflict drops; none
    // for a goal conflict.
    #[serde(skip)]
    pub staged: Option<usize>,
}

// The fields that make an update more than additive.
const ENTITY_REWRITES: [&str; 8] = [
    "name",
    "definition",
    "remove_aliases",
    "stereotype",
    "parent",
    "clear_parent",
    "set_attributes",
    "provenance",
];
const VIEW_REWRITES: [&str; 5] = ["title", "members", "query", "collapse", "reasoning"];

// Whether a mutation commutes with any other: creates reconcile by natural key, an
// additive entity or view update unions, a finding's id is its natural key.
pub fn commutes(mutation: &Value) -> bool {
    match mutation["op"].as_str().unwrap_or_default() {
        "create_entity" | "create_requirement" | "create_criterion" | "create_view"
        | "report_diagnostic" => true,
        "update_entity" => ENTITY_REWRITES.iter().all(|k| field_absent(mutation, k)),
        "update_view" => VIEW_REWRITES.iter().all(|k| field_absent(mutation, k)),
        _ => false,
    }
}

fn field_absent(mutation: &Value, key: &str) -> bool {
    match mutation.get(key) {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => !b,
        Some(Value::Array(a)) => a.is_empty(),
        _ => false,
    }
}

// What a mutation changes: node ids, and `doc#section` for section mutations.
fn targets(mutation: &Value) -> (Vec<String>, Vec<String>) {
    let s = |k: &str| mutation[k].as_str().map(str::to_string);
    let op = mutation["op"].as_str().unwrap_or_default();
    let mut nodes = Vec::new();
    let mut sections = Vec::new();
    match op {
        "merge_entities" => nodes.extend([s("keep"), s("absorb")].into_iter().flatten()),
        "set_coverage" => {
            if let (Some(doc), Some(section)) = (s("doc"), s("section")) {
                sections.push(format!("{doc}#{section}"));
            }
        }
        "classify_input" => nodes.extend(s("doc").map(|d| format!("input:{d}"))),
        // A prose edit carries its own staleness check (the old text must still stand).
        "edit_doc_prose" => {}
        _ => nodes.extend(s("id")),
    }
    (nodes, sections)
}

// Nodes a mutation removes outright.
fn removes(mutation: &Value) -> Vec<String> {
    let s = |k: &str| mutation[k].as_str().map(str::to_string);
    match mutation["op"].as_str().unwrap_or_default() {
        "delete_entity" | "delete_requirement" | "delete_criterion" | "delete_view"
        | "dissolve_entity" | "retract_decree" => s("id").into_iter().collect(),
        "merge_entities" => s("absorb").into_iter().collect(),
        _ => Vec::new(),
    }
}

struct Winner {
    generation: u64,
    kind: String,
    batch: Vec<String>,
    detail: String,
}

impl Store {
    // The conflicts between `ops`, staged against generation `since`, plus the goals
    // they resolve, and the commits after `since` up to this store's generation.
    pub fn conflicts_since(&self, since: u64, ops: &[Op], resolved: &[String]) -> Vec<Conflict> {
        let mut changed: BTreeMap<String, Winner> = BTreeMap::new();
        let mut sections: BTreeMap<String, Winner> = BTreeMap::new();
        let mut removed: BTreeMap<String, Winner> = BTreeMap::new();
        let mut goals: BTreeMap<String, Winner> = BTreeMap::new();
        for g in since + 1..=self.status.generation {
            let Some(entry) =
                crate::goals::read_journal_entry(&crate::layout::journal_file(&self.out, g), g)
            else {
                continue;
            };
            let winner = |detail: String| Winner {
                generation: g,
                kind: entry.kind.clone(),
                batch: entry.batch.clone(),
                detail,
            };
            for m in &entry.mutations {
                let op = m["op"].as_str().unwrap_or_default().to_string();
                for id in removes(m) {
                    removed.insert(id, winner(op.clone()));
                }
                if commutes(m) {
                    continue;
                }
                let (nodes, secs) = targets(m);
                for id in nodes {
                    changed.insert(id, winner(op.clone()));
                }
                for sec in secs {
                    sections.insert(sec, winner(op.clone()));
                }
            }
            for r in &entry.resolved_goals {
                goals.insert(r.goal.clone(), winner(r.justification.clone()));
            }
        }
        let conflict = |kind: &str, id: &str, w: &Winner, staged: Option<usize>| Conflict {
            kind: kind.into(),
            id: id.into(),
            generation: w.generation,
            by_kind: w.kind.clone(),
            by_batch: w.batch.clone(),
            detail: w.detail.clone(),
            staged,
        };
        let mut out = Vec::new();
        for (i, op) in ops.iter().enumerate() {
            let Ok(m) = serde_json::to_value(op) else {
                continue;
            };
            let (nodes, secs) = targets(&m);
            let additive = commutes(&m);
            let hit = nodes.iter().find_map(|id| {
                // A merged node is followed through its redirect; a deleted one is gone.
                if let Some(w) = removed.get(id) {
                    let redirected = self.graph.redirects.contains_key(id);
                    if !(additive && redirected) {
                        return Some(conflict("node", id, w, Some(i)));
                    }
                }
                (!additive)
                    .then(|| changed.get(id).map(|w| conflict("node", id, w, Some(i))))
                    .flatten()
            });
            let hit = hit.or_else(|| {
                secs.iter().find_map(|sec| {
                    sections
                        .get(sec)
                        .map(|w| conflict("section", sec, w, Some(i)))
                })
            });
            out.extend(hit);
        }
        for goal in resolved {
            if let Some(w) = goals.get(goal) {
                out.push(conflict("goal", goal, w, None));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn additive_mutations_commute_and_rewrites_do_not() {
        assert!(commutes(&json!({"op":"create_entity","id":"ent:a"})));
        assert!(commutes(
            &json!({"op":"update_entity","id":"ent:a","add_aliases":["A"],"add_mention":{"doc":"d.md"}})
        ));
        assert!(!commutes(
            &json!({"op":"update_entity","id":"ent:a","name":"B"})
        ));
        assert!(commutes(
            &json!({"op":"update_entity","id":"ent:a","clear_parent":false})
        ));
        assert!(!commutes(
            &json!({"op":"update_entity","id":"ent:a","parent":"ent:b"})
        ));
        assert!(commutes(
            &json!({"op":"update_view","id":"view:x","add_members":["ent:a"]})
        ));
        assert!(!commutes(
            &json!({"op":"update_view","id":"view:x","title":"T"})
        ));
        assert!(!commutes(
            &json!({"op":"update_requirement","id":"req:a","statement":"s"})
        ));
        assert!(commutes(&json!({"op":"report_diagnostic","id":"diag:x"})));
    }

    #[test]
    fn targets_name_nodes_and_sections() {
        assert_eq!(
            targets(&json!({"op":"merge_entities","keep":"ent:a","absorb":"ent:b"})).0,
            vec!["ent:a".to_string(), "ent:b".to_string()]
        );
        assert_eq!(
            targets(&json!({"op":"set_coverage","doc":"d.md","section":"/a"})).1,
            vec!["d.md#/a".to_string()]
        );
        assert_eq!(
            removes(&json!({"op":"merge_entities","keep":"ent:a","absorb":"ent:b"})),
            vec!["ent:b".to_string()]
        );
    }
}
