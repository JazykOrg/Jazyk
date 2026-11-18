//! Field-level audit of commits replayed on a newer graph.
use super::*;
use serde_json::{json, Value};

pub(crate) fn facts(store: &Store) -> BTreeMap<(String, String), Value> {
    fn fields(id: &str, prefix: &str, v: &Value, out: &mut BTreeMap<(String, String), Value>) {
        if let Some(map) = v.as_object() {
            for (k, v) in map {
                if matches!(k.as_str(), "created" | "updated") {
                    continue;
                }
                let p = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                fields(id, &p, v, out);
            }
        } else if prefix == "attributes" && v.is_array() {
            for a in v.as_array().unwrap() {
                fields(
                    id,
                    &format!("attributes.{}", a["name"].as_str().unwrap_or_default()),
                    a,
                    out,
                );
            }
        } else {
            out.insert((id.into(), prefix.into()), v.clone());
        }
    }
    let mut out = BTreeMap::new();
    let referenced = super::files::reference_sources(&store.graph);
    let mut entities = store.graph.entities.clone();
    for (id, entity) in &mut entities {
        if let Some(refs) = referenced.get(id) {
            entity.mentions.retain(|m| !refs.contains(m));
        }
    }
    for nodes in [
        serde_json::to_value(&entities),
        serde_json::to_value(&store.graph.requirements),
        serde_json::to_value(&store.graph.criteria),
        serde_json::to_value(&store.graph.views),
        serde_json::to_value(&store.graph.diagnostics),
    ] {
        if let Ok(Value::Object(nodes)) = nodes {
            for (id, node) in nodes {
                fields(&id, "", &node, &mut out);
            }
        }
    }
    for (doc, record) in &store.docs {
        for (sec, coverage) in &record.coverage {
            fields(
                &format!("{doc}#{sec}"),
                "coverage",
                &serde_json::to_value(coverage).unwrap_or_default(),
                &mut out,
            );
        }
    }
    if let Ok(Value::Object(categories)) = serde_json::to_value(&store.decisions) {
        for (category, entries) in categories {
            if let Some(entries) = entries.as_object() {
                for (id, value) in entries {
                    fields(id, &format!("decision.{category}"), value, &mut out);
                }
            }
        }
    }
    out
}
impl Store {
    pub(super) fn audit_fields(
        &self,
        before: &BTreeMap<(String, String), Value>,
        commit: &Commit,
    ) -> (Vec<Value>, Vec<Value>) {
        let after = facts(self);
        let keys: BTreeSet<_> = before.keys().chain(after.keys()).cloned().collect();
        let base = commit.base.as_ref().map(|s| facts(s));
        let missing = json!({"missing":true});
        let mut fields = Vec::new();
        let mut overwrites = Vec::new();
        for (id, field) in keys {
            let k = (id.clone(), field.clone());
            let prior = before.get(&k).unwrap_or(&missing);
            let next = after.get(&k).unwrap_or(&missing);
            if prior == next {
                continue;
            }
            fields.push(json!({"node":id,"field":field,"priorValue":prior,"value":next}));
            let Some(base) = base.as_ref() else { continue };
            let base_value = base.get(&k).unwrap_or(&missing);
            if base_value == prior {
                continue;
            }
            if [
                "aliases",
                "mentions",
                "entities",
                "members",
                "exclude",
                "collapse",
                "not_about",
            ]
            .contains(&field.as_str())
            {
                let empty = Vec::new();
                let base = base_value.as_array().unwrap_or(&empty);
                let prior = prior.as_array().unwrap_or(&empty);
                let next = next.as_array().unwrap_or(&empty);
                let added_survive = prior
                    .iter()
                    .filter(|v| !base.contains(v))
                    .all(|v| next.contains(v));
                let removed_stay_absent = base
                    .iter()
                    .filter(|v| !prior.contains(v))
                    .all(|v| !next.contains(v));
                if added_survive && removed_stay_absent {
                    continue;
                }
            }
            let mut generation = None;
            let mut author = None;
            let since = commit.base.as_ref().unwrap().status.generation;
            for g in (since + 1..self.status.generation).rev() {
                if let Some(e) =
                    crate::goals::read_journal_entry(&crate::layout::journal_file(&self.out, g), g)
                {
                    if e.field_changes
                        .iter()
                        .any(|f| f["node"] == id && f["field"] == field)
                    {
                        generation = Some(g);
                        author = e.actor.or_else(|| e.author.map(|a| json!({"name":a})));
                        break;
                    }
                }
            }
            overwrites.push(json!({"node":id,"field":field,"baseValue":base_value,"priorValue":prior,"value":next,"priorGeneration":generation,"priorAuthor":author.unwrap_or(json!("unknown"))}));
        }
        (fields, overwrites)
    }
    pub(super) fn overwrite_diagnostics(&mut self, overwrites: &[Value], generation: u64) {
        let nodes: BTreeSet<_> = overwrites
            .iter()
            .filter_map(|o| o["node"].as_str())
            .collect();
        for node in nodes {
            let details: Vec<_> = overwrites.iter().filter(|o| o["node"] == node).collect();
            let id = format!("diag:concurrent-overwrite-{generation}-{}", hash_hex(node));
            self.graph.diagnostics.insert(
                id,
                Diagnostic {
                    rule: "concurrent-overwrite".into(),
                    severity: "info".into(),
                    subjects: vec![node.into()],
                    message: format!(
                        "Concurrent commit overwrote {} field(s) on {node}",
                        details.len()
                    ),
                    reasoning: Some(serde_json::to_string(&details).unwrap_or_default()),
                    lifecycle: "open".into(),
                    triage: None,
                    prompt: None,
                    answer: None,
                    goal: None,
                    created: Some(format!("g{generation}")),
                    updated: Some(format!("g{generation}")),
                },
            );
        }
    }
}
