//! Read-only authored-node display while a session's mutations remain staged.
//! This is not a second store or a commit implementation.
use crate::{model::Graph, store::Op};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const MAX_BYTES: usize = 96 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preview {
    pub label: String,
    pub attempt: String,
    pub revision: u64,
    pub phase: String,
    pub generation: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

pub fn authored(graph: &Graph) -> Value {
    json!({"entities":graph.entities,"requirements":graph.requirements,"criteria":graph.criteria})
}

// Only the authored values already carried by accepted operations are displayed.
// Compound changes wait for the committed store projection rather than reproducing
// merge, provenance, or cascade semantics here.
pub fn staged(graph: &Graph, ops: &[Op]) -> Option<Value> {
    let mut result = authored(graph);
    for op in ops {
        let value = serde_json::to_value(op).ok()?;
        let name = value["op"].as_str()?;
        let id = value["id"].as_str().unwrap_or_default();
        let (collection, singular) = if name.ends_with("_entity") {
            ("entities", "entity")
        } else if name.ends_with("_requirement") {
            ("requirements", "requirement")
        } else if name.ends_with("_criterion") {
            ("criteria", "criterion")
        } else if matches!(
            name,
            "merge_entities"
                | "retract_decree"
                | "ratify_provenance"
                | "place_anchor"
                | "orphan_anchor"
        ) {
            return None;
        } else {
            continue;
        };
        if name.starts_with("create_") {
            result[collection][id] = value[singular].clone();
        } else if name.starts_with("delete_") {
            // Deletion cascades are store semantics, so wait for the commit.
            return None;
        } else if name.starts_with("update_") {
            let target = result[collection].get_mut(id)?.as_object_mut()?;
            for (key, field) in value.as_object()? {
                if key == "id" || key == "op" {
                    continue;
                }
                match key.as_str() {
                    "clear_parent" if field == true => {
                        target.remove("parent");
                    }
                    "clear_parent" => {}
                    "set_attributes" => {
                        target.insert("attributes".into(), field.clone());
                    }
                    "add_aliases" | "add_mention" | "not_about_add" | "add_attributes" => {
                        let dest = match key.as_str() {
                            "add_aliases" => "aliases",
                            "add_mention" => "mentions",
                            "not_about_add" => "not_about",
                            _ => "attributes",
                        };
                        let fields = field
                            .as_array()
                            .cloned()
                            .unwrap_or_else(|| vec![field.clone()]);
                        let values = target
                            .entry(dest.to_string())
                            .or_insert_with(|| json!([]))
                            .as_array_mut()?;
                        for item in fields {
                            if dest == "attributes" {
                                if let Some(existing) =
                                    values.iter_mut().find(|v| v["name"] == item["name"])
                                {
                                    *existing = item;
                                    continue;
                                }
                            }
                            if !values.contains(&item) {
                                values.push(item);
                            }
                        }
                    }
                    "remove_aliases" => {
                        if let Some(values) =
                            target.get_mut("aliases").and_then(Value::as_array_mut)
                        {
                            values.retain(|v| {
                                !field.as_array().is_some_and(|removed| removed.contains(v))
                            });
                        }
                    }
                    _ => {
                        target.insert(key.clone(), field.clone());
                    }
                }
            }
        } else {
            return None;
        }
    }
    Some(result)
}

impl Preview {
    pub fn bounded(mut self) -> Self {
        if serde_json::to_vec(&self).map_or(true, |v| v.len() > MAX_BYTES) {
            self.graph = None;
            self.reason =
                Some("Live preview paused because this graph exceeds the display limit.".into());
        }
        self
    }
}

fn path(out: &Path, label: &str) -> PathBuf {
    out.join("preview")
        .join(format!("{}.json", crate::model::hash_hex(label)))
}

// Native ACP's serving is another process. These private, ephemeral display bytes
// never enter the hosted or browser workspace snapshots.
pub fn write(out: &Path, preview: &Preview) {
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;
        let file = path(out, &preview.label);
        let dir = file.parent().unwrap();
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        #[cfg(unix)]
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        let temp = file.with_extension(format!("{}.tmp", std::process::id()));
        if std::fs::write(&temp, serde_json::to_vec(preview).unwrap_or_default()).is_ok() {
            #[cfg(unix)]
            let _ = std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600));
            let _ = std::fs::rename(&temp, file);
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (out, preview);
}

pub fn read(out: &Path, label: &str) -> Option<Preview> {
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() {
        let file = path(out, label);
        if std::fs::metadata(&file).ok()?.len() > MAX_BYTES as u64 {
            return None;
        }
        let value: Preview = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
        return (value.label == label).then_some(value);
    }
    let _ = (out, label);
    None
}

pub fn clear(out: &Path, label: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() {
        let _ = std::fs::remove_file(path(out, label));
    }
    let _ = (out, label);
}

pub struct SidecarGuard(PathBuf, String);
impl SidecarGuard {
    pub fn start(out: &Path, label: &str) -> Self {
        clear(out, label);
        Self(out.to_path_buf(), label.into())
    }
}
impl Drop for SidecarGuard {
    fn drop(&mut self) {
        clear(&self.0, &self.1);
    }
}

pub fn result_value(value: &Value) -> Value {
    if let Some(text) = value.as_str() {
        return serde_json::from_str(text).unwrap_or_else(|_| value.clone());
    }
    if let Some(text) = value["content"][0]["text"].as_str() {
        return serde_json::from_str(text).unwrap_or_else(|_| value.clone());
    }
    value.clone()
}

/// Plain activity from the structured invocation. Never invent a successful result.
pub fn activity(name: &str, args: &Value, result: Option<&Value>) -> String {
    let subject = args["name"]
        .as_str()
        .or_else(|| args["statement"].as_str())
        .or_else(|| args["title"].as_str())
        .or_else(|| args["id"].as_str())
        .or_else(|| args["target"].as_str())
        .unwrap_or_default();
    let subject = crate::llm::truncate(subject, 100);
    let noun = if name.contains("entity") {
        "Entity"
    } else if name.contains("requirement") {
        "Requirement"
    } else if name.contains("criterion") {
        "Criterion"
    } else {
        "Item"
    };
    let subject = if subject.is_empty() { noun } else { &subject };
    if name.starts_with("upsert_") || name.starts_with("update_") {
        match result {
            Some(r) if r.get("error").is_some() || r["isError"] == true => {
                format!("{subject} change refused")
            }
            Some(r) if r["created"] == true => format!("{subject} created"),
            Some(r) if r["updated"] == true || r["created"] == false => {
                format!("{subject} updated")
            }
            Some(_) => format!("{subject} accepted"),
            None => format!("Updating {subject}…"),
        }
    } else if name.starts_with("delete_") {
        if result.is_some() {
            format!("{subject} deletion staged")
        } else {
            format!("Deleting {subject}…")
        }
    } else if name == "set_coverage" {
        let section = args["section"].as_str().unwrap_or("section");
        let state = args["state"].as_str().unwrap_or("reviewed");
        if result.is_some() {
            format!("{section} marked {state}")
        } else {
            format!("Marking {section} {state}…")
        }
    } else if name == "mark_goal_done" {
        if result.is_some() {
            args["justification"]
                .as_str()
                .map(|s| crate::llm::truncate(s, 160))
                .unwrap_or_else(|| "Review completed".into())
        } else {
            "Completing review…".into()
        }
    } else if name == "search" {
        let query = args["query"]
            .as_str()
            .or_else(|| args["text"].as_str())
            .unwrap_or("");
        format!("Searching {}", crate::llm::truncate(query, 100))
            .trim()
            .into()
    } else if name == "done" {
        if result.is_some() {
            "Session saved".into()
        } else {
            "Saving session…".into()
        }
    } else {
        let verb = match name {
            "load" | "expand" | "read_section" | "read_doc" | "get_entity" => "Reading",
            "search" | "lookup_entity" => "Searching",
            "mark_goal_done" => "Completing review",
            "mark_goal_failed" => "Recording a blocked review",
            "done" => "Saving session",
            "report_diagnostic" => "Recording a finding",
            _ => "Running",
        };
        if verb == "Running" {
            name.replace('_', " ")
        } else if subject == noun {
            verb.into()
        } else {
            format!("{verb} {subject}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Entity, Requirement};
    #[test]
    fn accepted_facts_are_provisional_and_do_not_mutate_the_base() {
        let graph = Graph::default();
        let ops = vec![
            Op::CreateEntity {
                id: "ent:person".into(),
                entity: Entity {
                    name: "Person".into(),
                    ..Default::default()
                },
            },
            Op::CreateRequirement {
                id: "req:1".into(),
                requirement: Requirement {
                    statement: "A person signs in".into(),
                    entities: vec!["ent:person".into()],
                    ..Default::default()
                },
            },
        ];
        let preview = staged(&graph, &ops).unwrap();
        assert_eq!(preview["entities"]["ent:person"]["name"], "Person");
        assert_eq!(
            preview["requirements"]["req:1"]["entities"][0],
            "ent:person"
        );
        assert!(graph.entities.is_empty());
        assert!(graph.requirements.is_empty());
        assert!(staged(
            &graph,
            &[Op::MergeEntities {
                keep: "ent:a".into(),
                absorb: "ent:b".into(),
                reason: "same".into()
            }]
        )
        .is_none());
    }
    #[test]
    fn successful_wording_requires_structured_result() {
        let args = json!({"name":"Person"});
        assert_eq!(activity("upsert_entity", &args, None), "Updating Person…");
        assert_eq!(
            activity("upsert_entity", &args, Some(&json!({"created":true}))),
            "Person created"
        );
        assert_eq!(
            activity("upsert_entity", &args, Some(&json!({"created":false}))),
            "Person updated"
        );
    }
}
