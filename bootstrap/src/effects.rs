// What a mutating chat tool changed, as one structured record: the file hunks it
// wrote and the graph nodes it added, updated, or removed. Every tool reply that
// commits carries it as `effects`; the ACP layers decorate the tool call update
// from it with standard diff content and locations plus `_meta.jazyk.effects`.
// Mirrors docs/frontends/acp.md#edit-effects.

use crate::model::Graph;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

// More graph effects than this are cut, with the count of the rest.
const GRAPH_CAP: usize = 40;

// Bookkeeping, not facts: the generation markers and the mentions an entity takes
// from its requirements. A node whose only change is one of these is left out.
const QUIET_FIELDS: [&str; 3] = ["created", "updated", "mentions"];

// The product word for a path: prose documents, the configuration, and anything
// else as a source or test file by its name.
pub fn file_kind(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with("jazyk.toml") {
        "config"
    } else if lower.ends_with(".md")
        || lower.ends_with(".markdown")
        || lower.ends_with(".txt")
        || crate::asciidoc::supported(Path::new(path))
        || crate::rst::supported(Path::new(path))
    {
        "doc"
    } else if lower.contains("test") || lower.contains("spec") {
        "test"
    } else {
        "source"
    }
}

// The changed lines between two texts: the common leading and trailing lines cut,
// what remains as old and new, and the 1-based line the change starts at in the
// new text. None when the texts are equal.
pub fn hunk(old: &str, new: &str) -> Option<(usize, String, String)> {
    if old == new {
        return None;
    }
    let a: Vec<&str> = old.split_inclusive('\n').collect();
    let b: Vec<&str> = new.split_inclusive('\n').collect();
    let mut pre = 0;
    while pre < a.len() && pre < b.len() && a[pre] == b[pre] {
        pre += 1;
    }
    let mut post = 0;
    while post < a.len() - pre
        && post < b.len() - pre
        && a[a.len() - 1 - post] == b[b.len() - 1 - post]
    {
        post += 1;
    }
    let old_mid = a[pre..a.len() - post].concat();
    let new_mid = b[pre..b.len() - post].concat();
    Some((pre + 1, old_mid, new_mid))
}

// One file effect: the hunk an edit made to one file. `old` is None for a new file.
pub fn file_effect(root: &Path, rel: &str, old: Option<&str>, new: &str) -> Option<Value> {
    let (line, old_text, new_text) = hunk(old.unwrap_or_default(), new)?;
    Some(json!({
        "path": rel,
        "abs": root.join(rel).display().to_string(),
        "kind": file_kind(rel),
        "line": line,
        "created": old.is_none(),
        "oldText": old_text,
        "newText": new_text,
    }))
}

fn title_of(kind: &str, v: &Value) -> String {
    if kind == "relationship" {
        let members: Vec<&str> = v["members"]
            .as_array()
            .map(|a| a.iter().filter_map(|m| m.as_str()).collect())
            .unwrap_or_default();
        return members.join(" ↔ ");
    }
    for key in [
        "statement",
        "name",
        "title",
        "message",
        "condition",
        "subject",
    ] {
        if let Some(s) = v[key].as_str().filter(|s| !s.is_empty()) {
            return s.to_string();
        }
    }
    String::new()
}

fn diff_map<T: serde::Serialize>(
    kind: &str,
    before: &BTreeMap<String, T>,
    after: &BTreeMap<String, T>,
    skip: &dyn Fn(&Value) -> bool,
    out: &mut Vec<Value>,
) {
    let to_value = |t: &T| serde_json::to_value(t).unwrap_or(Value::Null);
    for (id, a) in after {
        let av = to_value(a);
        match before.get(id) {
            None => {
                if skip(&av) {
                    continue;
                }
                out.push(json!({"op": "added", "kind": kind, "id": id, "title": title_of(kind, &av), "node": av}));
            }
            Some(b) => {
                let bv = to_value(b);
                if bv == av || (skip(&av) && skip(&bv)) {
                    continue;
                }
                let empty = Map::new();
                let (bo, ao) = (
                    bv.as_object().unwrap_or(&empty),
                    av.as_object().unwrap_or(&empty),
                );
                let mut keys: Vec<&String> = bo.keys().chain(ao.keys()).collect();
                keys.sort();
                keys.dedup();
                let changes: Vec<Value> = keys
                    .into_iter()
                    .filter(|k| !QUIET_FIELDS.contains(&k.as_str()) && bo.get(*k) != ao.get(*k))
                    .map(|k| json!({"field": k, "before": bo.get(k).cloned().unwrap_or(Value::Null), "after": ao.get(k).cloned().unwrap_or(Value::Null)}))
                    .collect();
                if changes.is_empty() {
                    continue;
                }
                out.push(json!({"op": "updated", "kind": kind, "id": id, "title": title_of(kind, &av), "changes": changes}));
            }
        }
    }
    for (id, b) in before {
        if after.contains_key(id) {
            continue;
        }
        let bv = to_value(b);
        if skip(&bv) {
            continue;
        }
        out.push(json!({"op": "removed", "kind": kind, "id": id, "title": title_of(kind, &bv), "node": bv}));
    }
}

// The graph effects of one commit: every authored and derived node that differs
// between the two graphs, default views left out (they follow the rest).
pub fn graph_effects(before: &Graph, after: &Graph) -> Vec<Value> {
    let none = |_: &Value| false;
    let default_view = |v: &Value| v["default"] == true;
    let mut out = Vec::new();
    diff_map(
        "requirement",
        &before.requirements,
        &after.requirements,
        &none,
        &mut out,
    );
    diff_map("entity", &before.entities, &after.entities, &none, &mut out);
    diff_map(
        "criterion",
        &before.criteria,
        &after.criteria,
        &none,
        &mut out,
    );
    diff_map(
        "relationship",
        &before.relationships,
        &after.relationships,
        &none,
        &mut out,
    );
    diff_map(
        "state-machine",
        &before.state_machines,
        &after.state_machines,
        &none,
        &mut out,
    );
    diff_map("view", &before.views, &after.views, &default_view, &mut out);
    diff_map(
        "diagnostic",
        &before.diagnostics,
        &after.diagnostics,
        &none,
        &mut out,
    );
    out
}

// The `effects` record a tool reply carries. Empty parts are left out; None when
// nothing changed.
pub fn record(files: Vec<Value>, mut graph: Vec<Value>) -> Option<Value> {
    if files.is_empty() && graph.is_empty() {
        return None;
    }
    let mut v = json!({});
    if !files.is_empty() {
        v["files"] = json!(files);
    }
    if graph.len() > GRAPH_CAP {
        v["truncated"] = json!(graph.len() - GRAPH_CAP);
        graph.truncate(GRAPH_CAP);
    }
    if !graph.is_empty() {
        v["graph"] = json!(graph);
    }
    Some(v)
}

// The effects a tool output carries, whatever wrapped it: the reply object itself,
// its JSON text, or an MCP result's text content (an external agent's rawOutput).
pub fn find(output: &Value) -> Option<Value> {
    match output {
        Value::String(s) => serde_json::from_str::<Value>(s).ok().and_then(|v| find(&v)),
        Value::Object(o) => {
            if let Some(e) = o.get("effects").filter(|e| e.is_object()) {
                return Some(e.clone());
            }
            if let Some(e) = o.get("structuredContent").and_then(find) {
                return Some(e);
            }
            o.get("content").and_then(find)
        }
        Value::Array(items) => items.iter().find_map(|i| {
            i["text"]
                .as_str()
                .and_then(|t| find(&Value::String(t.into())))
        }),
        _ => None,
    }
}

fn short(v: &Value) -> String {
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Null => "none".into(),
        other => other.to_string(),
    };
    let s = s.replace('\n', " ");
    if s.chars().count() > 120 {
        format!("{}…", s.chars().take(119).collect::<String>())
    } else {
        s
    }
}

// The portable reading of graph effects: one markdown line per change, what any
// ACP client shows when it does not read `_meta`.
pub fn summary(effects: &Value) -> String {
    let mut lines = Vec::new();
    for g in effects["graph"].as_array().into_iter().flatten() {
        let kind = g["kind"].as_str().unwrap_or("node");
        let id = g["id"].as_str().unwrap_or_default();
        let title = g["title"].as_str().unwrap_or_default();
        let head = match g["op"].as_str() {
            Some("added") => format!("+ New {kind}"),
            Some("removed") => format!("− Removed {kind}"),
            _ => format!("~ Changed {kind}"),
        };
        let mut line = format!("- **{head}** `{id}`");
        if !title.is_empty() {
            line.push_str(&format!(": {}", short(&json!(title))));
        }
        lines.push(line);
        for c in g["changes"].as_array().into_iter().flatten() {
            lines.push(format!(
                "  - `{}`: {} → {}",
                c["field"].as_str().unwrap_or_default(),
                short(&c["before"]),
                short(&c["after"])
            ));
        }
    }
    if let Some(n) = effects["truncated"].as_u64() {
        lines.push(format!("- and {n} more graph changes"));
    }
    if lines.is_empty() {
        return String::new();
    }
    format!("**Graph changes**\n{}", lines.join("\n"))
}

// Decorate one ACP `tool_call_update` (JSON) whose output carries effects: `kind`
// edit, a diff content block per file hunk, a markdown block for the graph,
// locations for each file (0-based line), and `_meta.jazyk.effects`. An update
// without effects is left as it is.
pub fn decorate(update: &mut Value) {
    if update["sessionUpdate"] != "tool_call_update" && update.get("sessionUpdate").is_some() {
        return;
    }
    if update["status"] != "completed" {
        return;
    }
    let Some(effects) = find(&update["rawOutput"]) else {
        return;
    };
    let mut content = Vec::new();
    let mut locations = Vec::new();
    for f in effects["files"].as_array().into_iter().flatten() {
        let abs = f["abs"].as_str().or(f["path"].as_str()).unwrap_or_default();
        let mut diff = json!({"type": "diff", "path": abs, "newText": f["newText"]});
        if f["created"] != true {
            diff["oldText"] = f["oldText"].clone();
        }
        content.push(diff);
        let line = f["line"].as_u64().unwrap_or(1).saturating_sub(1);
        locations.push(json!({"path": abs, "line": line}));
    }
    let text = summary(&effects);
    if !text.is_empty() {
        content.push(json!({"type": "content", "content": {"type": "text", "text": text}}));
    }
    update["kind"] = json!("edit");
    if !content.is_empty() {
        update["content"] = json!(content);
    }
    if !locations.is_empty() {
        update["locations"] = json!(locations);
    }
    update["_meta"]["jazyk"]["effects"] = effects;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Requirement;

    fn req(statement: &str, entities: &[&str]) -> Requirement {
        serde_json::from_value(json!({"statement": statement, "entities": entities})).unwrap()
    }

    #[test]
    fn hunk_cuts_common_lines() {
        let (line, old, new) = hunk("a\nb\nc\n", "a\nB\nc\n").unwrap();
        assert_eq!((line, old.as_str(), new.as_str()), (2, "b\n", "B\n"));
        let (line, old, new) = hunk("a\nc\n", "a\nb\nc\n").unwrap();
        assert_eq!((line, old.as_str(), new.as_str()), (2, "", "b\n"));
        assert!(hunk("same", "same").is_none());
    }

    #[test]
    fn graph_effects_name_added_updated_removed() {
        let mut before = Graph::default();
        before
            .requirements
            .insert("req:a".into(), req("Orders ship.", &["ent:order"]));
        before
            .requirements
            .insert("req:b".into(), req("Carts expire.", &["ent:cart"]));
        let mut after = Graph::default();
        after
            .requirements
            .insert("req:a".into(), req("Orders ship in a day.", &["ent:order"]));
        after
            .requirements
            .insert("req:c".into(), req("Refunds are logged.", &["ent:refund"]));
        let e = graph_effects(&before, &after);
        let ops: Vec<(&str, &str)> = e
            .iter()
            .map(|g| (g["op"].as_str().unwrap(), g["id"].as_str().unwrap()))
            .collect();
        assert_eq!(
            ops,
            [
                ("updated", "req:a"),
                ("added", "req:c"),
                ("removed", "req:b")
            ]
        );
        assert_eq!(e[0]["changes"][0]["field"], "statement");
        assert_eq!(e[0]["changes"][0]["after"], "Orders ship in a day.");
    }

    #[test]
    fn decorate_adds_standard_acp_fields() {
        let effects = record(
            vec![file_effect(Path::new("/p"), "docs/a.md", Some("x\nold\n"), "x\nnew\n").unwrap()],
            vec![json!({"op":"updated","kind":"requirement","id":"req:a","title":"T","changes":[{"field":"statement","before":"old","after":"new"}]})],
        )
        .unwrap();
        let reply = json!({"committed": true, "effects": effects});
        let mut u = json!({"sessionUpdate":"tool_call_update","toolCallId":"1","status":"completed",
            "rawOutput": serde_json::to_string(&reply).unwrap()});
        decorate(&mut u);
        assert_eq!(u["kind"], "edit");
        assert_eq!(u["content"][0]["type"], "diff");
        assert_eq!(u["content"][0]["path"], "/p/docs/a.md");
        assert_eq!(u["content"][0]["oldText"], "old\n");
        assert_eq!(u["locations"][0]["line"], 1);
        assert!(u["content"][1]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("Changed requirement"));
        assert_eq!(u["_meta"]["jazyk"]["effects"]["files"][0]["kind"], "doc");
        // The same reply wrapped as an external agent's MCP result decorates too.
        let mut ext = json!({"sessionUpdate":"tool_call_update","toolCallId":"2","status":"completed",
            "rawOutput": {"content":[{"type":"text","text": serde_json::to_string(&reply).unwrap()}]}});
        decorate(&mut ext);
        assert_eq!(ext["kind"], "edit");
    }
}
