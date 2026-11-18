//! Explicit human diagram edits reuse the writable-chat validators and commit.
use crate::{mcp::McpServer, project::Project, store::Store};
use serde_json::{json, Value};
use std::path::Path;

pub fn coverage(store: &Store) -> Value {
    json!({"findings": crate::diagrams::findings(store).iter().map(|f| {
        let mut v = f.value();
        v["areaHash"] = json!(f.hash());
        v["covered"] = json!(f.covered(store));
        v["suppressed"] = json!(f.suppressed(store));
        v["mandatory"] = json!(f.family == "source" || f.count() > f.hard);
        v
    }).collect::<Vec<_>>()})
}

pub fn edit(proj: &Project, out: &Path, body: &Value) -> Result<Value, super::Refusal> {
    let tool = body["tool"].as_str().unwrap_or_default();
    if !matches!(
        tool,
        "upsert_view"
            | "update_view"
            | "delete_view"
            | "suppress_diagram_coverage"
            | "unsuppress_diagram_coverage"
    ) {
        return Err((
            400,
            "Choose a diagram or diagram suppression operation".into(),
        ));
    }
    let args = &body["arguments"];
    if !args.is_object() || args.get("setId").is_some() {
        return Err((
            400,
            "arguments must be an object in the selected document set".into(),
        ));
    }
    if matches!(
        tool,
        "upsert_view" | "update_view" | "suppress_diagram_coverage"
    ) && args["decision"]
        .as_str()
        .is_none_or(|s| s.trim().is_empty())
    {
        return Err((400, "A human decision note is required".into()));
    }
    let server = McpServer::new(proj.clone(), out.into(), vec!["chat".into()], true);
    let reply = server.commit_graph_edits(&[(tool.into(), args.clone())]);
    if !reply["error"].is_null() {
        let status = if reply["error"]["rule"] == "area-moved" {
            409
        } else {
            400
        };
        return Err((
            status,
            reply["error"]["message"]
                .as_str()
                .unwrap_or("Diagram edit refused")
                .into(),
        ));
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Entity, Provenance, Requirement, VIEW_KINDS};

    fn fixture(name: &str) -> (Project, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "jazyk-diagram-api-{name}-{}",
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&root).ok();
        crate::vfs::create_dir_all(&root).unwrap();
        let out = root.join(".jazyk/test");
        let mut store = Store::load(&out);
        for i in 0..10 {
            store.graph.entities.insert(
                format!("ent:item-{i}"),
                Entity {
                    name: format!("Item {i}"),
                    provenance: Some(Provenance::Decree {
                        author: "fixture".into(),
                        at: "2026-11-18T00:00:00Z".into(),
                        note: Some("An explicitly authored fixture entity".into()),
                    }),
                    ..Default::default()
                },
            );
        }
        store.graph.requirements.insert(
            "req:message".into(),
            Requirement {
                statement: "Item zero sends a message to item one.".into(),
                entities: vec!["ent:item-0".into(), "ent:item-1".into()],
                ..Default::default()
            },
        );
        store.save_for_test();
        (
            Project {
                root,
                out: out.clone(),
                docs_glob: vec![],
                ..Default::default()
            },
            out,
        )
    }

    #[test]
    fn human_api_authors_updates_and_deletes_every_kind_with_the_tool_gates() {
        let (proj, out) = fixture("all-kinds");
        for kind in VIEW_KINDS {
            let members = if [
                "class",
                "component",
                "package",
                "object",
                "composite",
                "deployment",
                "state",
            ]
            .contains(&kind)
            {
                json!(["ent:item-0"])
            } else if kind == "timing" {
                json!(["ent:item-0", "req:message"])
            } else {
                json!(["req:message"])
            };
            let create = json!({"tool":"upsert_view","arguments":{"kind":kind,"title":format!("Example {kind}"),"members":members,"purpose":"Explain one area","area":{"target":"scope:public","extent":"selection"},"reasoning":"Explicit human diagram","decision":"Show this selection"}});
            let result = edit(&proj, &out, &create).unwrap();
            assert_eq!(result["committed"], true);
            let id = result["id"].as_str().unwrap();
            let changed = edit(&proj, &out, &json!({"tool":"update_view","arguments":{"id":id,"title":format!("Edited {kind}"),"reasoning":"Clarify the title","decision":"Use this title"}})).unwrap();
            assert_eq!(changed["committed"], true);
            let saved = Store::load(&out);
            assert!(matches!(
                saved.graph.views[id].provenance,
                Some(Provenance::Decree { .. })
            ));
            let before = saved.status.generation;
            assert!(edit(&proj, &out, &json!({"tool":"update_view","arguments":{"id":id,"members":["ent:missing"],"decision":"Use an invalid member","reasoning":"Test the gate"}})).is_err());
            assert_eq!(Store::load(&out).status.generation, before);
            edit(
                &proj,
                &out,
                &json!({"tool":"delete_view","arguments":{"id":id,"reason":"No longer needed"}}),
            )
            .unwrap();
            assert!(!Store::load(&out).graph.views.contains_key(id));
        }
        assert!(edit(&proj, &out, &json!({"tool":"delete_entity","arguments":{"id":"ent:item-0","reason":"Wrong surface"}})).is_err());
        assert!(edit(&proj, &out, &json!({"tool":"upsert_view","arguments":{"kind":"class","title":"No decision","reasoning":"Missing human choice"}})).is_err());
        crate::vfs::remove_dir_all(&proj.root).ok();
    }

    #[test]
    fn human_api_reads_suppresses_updates_and_removes_decisions_and_refuses_stale_areas() {
        let (proj, out) = fixture("decisions");
        let finding = coverage(&Store::load(&out))["findings"][0].clone();
        assert_eq!(finding["mandatory"], true);
        let mut body = json!({"tool":"suppress_diagram_coverage","arguments":{"id":finding["id"],"expectedAreaHash":finding["areaHash"],"reason":"An ordered list answers this question","decision":"Keep these facts as a list"}});
        edit(&proj, &out, &body).unwrap();
        let saved = crate::diagrams::suppressions(&Store::load(&out));
        assert_eq!(saved["suppressions"][0]["active"], true);
        body["arguments"]["reason"] =
            json!("The glossary already explains these independent names");
        edit(&proj, &out, &body).unwrap();
        assert_eq!(
            crate::diagrams::suppressions(&Store::load(&out))["suppressions"][0]["decision"]
                ["reason"],
            body["arguments"]["reason"]
        );
        body["arguments"]["expectedAreaHash"] = json!("stale");
        assert_eq!(edit(&proj, &out, &body).unwrap_err().0, 409);
        edit(&proj, &out, &json!({"tool":"unsuppress_diagram_coverage","arguments":{"id":finding["id"],"reason":"Reconsider the diagram"}})).unwrap();
        assert_eq!(
            crate::diagrams::suppressions(&Store::load(&out))["suppressions"],
            json!([])
        );
        crate::vfs::remove_dir_all(&proj.root).ok();
    }

    #[test]
    fn browser_rendering_uses_shared_preparation_and_live_element_destinations() {
        let (proj, out) = fixture("browser-source");
        let result = edit(&proj, &out, &json!({"tool":"upsert_view","arguments":{"kind":"component","title":"One component","members":["ent:item-0"],"purpose":"Explain one component","decision":"Show this component","reasoning":"A human selection"}})).unwrap();
        let saved = Store::load(&out);
        let id = result["id"].as_str().unwrap();
        let source = crate::render::browser_source(&saved, id, &saved.graph.views[id]);
        assert!(source["puml"].as_str().unwrap().contains("@startuml"));
        assert!(!source["puml"].as_str().unwrap().contains(" [[../"));
        assert!(!source["links"].as_array().unwrap().is_empty());
        assert!(source["links"][0][1]
            .as_str()
            .unwrap()
            .contains("docsgen/entities/"));
        assert_eq!(
            source,
            crate::render::browser_source(&saved, id, &saved.graph.views[id])
        );
        crate::vfs::remove_dir_all(&proj.root).ok();
    }

    #[test]
    fn editor_can_remove_all_prior_exclusions() {
        let (proj, out) = fixture("exclusions");
        let result = edit(&proj, &out, &json!({"tool":"upsert_view","arguments":{"kind":"class","title":"Selection","members":["ent:item-0"],"excluded":[{"id":"ent:item-1","note":"Outside the question"}],"notes":[{"text":"A temporary note"}],"collapse":["ent:item-0"],"purpose":"Explain a selection","decision":"Show item zero","reasoning":"A human selection"}})).unwrap();
        let id = result["id"].as_str().unwrap();
        edit(&proj, &out, &json!({"tool":"update_view","arguments":{"id":id,"clear_excluded":true,"exclude":[],"members":[],"collapse":[],"notes":[],"decision":"Allow excluded members again","reasoning":"Review the area"}})).unwrap();
        let saved = Store::load(&out);
        assert!(saved.graph.views[id].excluded.is_empty());
        assert!(saved.graph.views[id].members.is_empty());
        assert!(saved.graph.views[id].collapse.is_empty());
        assert_eq!(saved.graph.views[id].authored["notes"], json!([]));
        assert!(saved.graph.entities.contains_key("ent:item-0"));
        crate::vfs::remove_dir_all(&proj.root).ok();
    }
}
