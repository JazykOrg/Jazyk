// Generation state and task packages: the shared contract between every generation
// worker, the built-in `jazyk codegen` and external MCP agents alike. Mirrors
// docs2/compiler/tools.md#generation-tools.
use crate::model::hash_hex;
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

// Requirements per generation part for dense entities.
pub const GROUP: usize = 20;

pub fn ext_for(lang: &str) -> &'static str {
    match lang {
        "rust" => "rs",
        "python" => "py",
        "typescript" => "ts",
        "go" => "go",
        _ => "txt",
    }
}

// The generation contract, identical for every worker.
pub fn instructions(lang: &str) -> String {
    format!(
        "Generate ONE self-contained {lang} code unit per entity from its specification.\n\
         - Start the file with a comment header naming the entity id and the requirement ids implemented (the traceability key).\n\
         - Every requirement is an obligation; implement each and cite its id in a comment at the implementing site.\n\
         - Reference other units by their entity slug as module or import names when a relationship requires them.\n\
         - Dense entities generate in parts of {GROUP} requirements: part 1 is the module's types, state, and the first group; each later part receives the module so far and returns ONLY additional code to append (further impl blocks and helpers), without repeating existing items.\n\
         - Return only code, never fences or prose."
    )
}

// Generation state: slug -> (fact hash, requirement ids at generation time). Earlier
// versions stored a bare hash string; both forms load.
pub struct GenState {
    map: BTreeMap<String, (String, Vec<String>)>,
    path: PathBuf,
}

impl GenState {
    pub fn load(out: &std::path::Path) -> GenState {
        let path = out.join("codegen").join("state.yaml");
        let mut map = BTreeMap::new();
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(v) = serde_norway::from_str::<BTreeMap<String, Value>>(&text) {
                for (slug, entry) in v {
                    match entry {
                        Value::String(h) => {
                            map.insert(slug, (h, Vec::new()));
                        }
                        Value::Object(o) => {
                            let h = o.get("hash").and_then(|x| x.as_str()).unwrap_or_default().to_string();
                            let reqs = o
                                .get("requirements")
                                .and_then(|x| x.as_array())
                                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                                .unwrap_or_default();
                            map.insert(slug, (h, reqs));
                        }
                        _ => {}
                    }
                }
            }
        }
        GenState { map, path }
    }

    pub fn save(&self) {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let v: BTreeMap<&String, Value> = self
            .map
            .iter()
            .map(|(k, (h, r))| (k, json!({"hash": h, "requirements": r})))
            .collect();
        if let Ok(text) = serde_norway::to_string(&v) {
            std::fs::write(&self.path, text).ok();
        }
    }

    pub fn get(&self, slug: &str) -> Option<&(String, Vec<String>)> {
        self.map.get(slug)
    }

    pub fn mark(&mut self, slug: &str, hash: String, requirements: Vec<String>) {
        self.map.insert(slug.to_string(), (hash, requirements));
    }
}

pub fn slug_of(id: &str) -> String {
    id.strip_prefix("ent:").unwrap_or(id).to_string()
}

pub fn reqs_of_sorted(store: &Store, id: &str) -> Vec<String> {
    let mut v = store.requirements_referencing(id);
    v.sort();
    v
}

pub fn fact_hash(store: &Store, id: &str) -> String {
    let e = &store.graph.entities[id];
    let mut facts = format!("{}|{}|", e.name, e.definition.as_deref().unwrap_or(""));
    for rid in reqs_of_sorted(store, id) {
        if let Some(r) = store.graph.requirements.get(&rid) {
            facts.push_str(&r.ears);
            facts.push('|');
        }
    }
    hash_hex(&facts)
}

// The change diff for one entity versus its generation state.
fn change_diff(state: &GenState, slug: &str, current: &[String]) -> (String, Vec<String>) {
    match state.get(slug) {
        None => ("new".to_string(), current.iter().map(|r| format!("{} (added)", r)).collect()),
        Some((_, old)) => {
            let mut changed: Vec<String> = Vec::new();
            for r in current {
                if !old.contains(r) {
                    changed.push(format!("{} (added)", r));
                }
            }
            for r in old {
                if !current.contains(r) {
                    changed.push(format!("{} (removed)", r));
                }
            }
            if changed.is_empty() {
                changed.push("(reworded: same requirement set, changed statements or definition)".to_string());
            }
            ("changed".to_string(), changed)
        }
    }
}

// Entities whose facts differ from the generation state.
pub fn pending(store: &Store, lang: &str) -> Vec<Value> {
    let state = GenState::load(&store.out);
    let ext = ext_for(lang);
    let mut out = Vec::new();
    for id in store.graph.entities.keys() {
        let rids = reqs_of_sorted(store, id);
        if rids.is_empty() {
            continue;
        }
        let slug = slug_of(id);
        let hash = fact_hash(store, id);
        let unit = store.out.join("codegen").join(format!("{}.{}", slug, ext));
        if state.get(&slug).map(|(h, _)| h) == Some(&hash) && unit.exists() {
            continue;
        }
        let (reason, changed) = change_diff(&state, &slug, &rids);
        out.push(json!({
            "entity": id,
            "unit": unit.to_string_lossy(),
            "reason": reason,
            "changed": changed,
        }));
    }
    out
}

// The full package a worker needs to generate one unit.
pub fn task_package(store: &Store, id: &str, lang: &str) -> Result<Value, String> {
    if !store.graph.entities.contains_key(id) {
        return Err(format!("unknown entity `{}`", id));
    }
    let state = GenState::load(&store.out);
    let e = &store.graph.entities[id];
    let slug = slug_of(id);
    let rids = reqs_of_sorted(store, id);
    let (_, changed) = change_diff(&state, &slug, &rids);
    let groups: Vec<Vec<Value>> = rids
        .chunks(GROUP)
        .map(|chunk| {
            chunk
                .iter()
                .filter_map(|rid| {
                    store.graph.requirements.get(rid).map(|r| json!({"id": rid, "ears": r.ears}))
                })
                .collect()
        })
        .collect();
    let rels: Vec<String> = store
        .graph
        .relationships
        .values()
        .filter(|rel| rel.members.iter().any(|m| m == id))
        .map(|rel| {
            format!(
                "{} {}",
                rel.rel_type,
                rel.members.iter().filter(|m| *m != id).cloned().collect::<Vec<_>>().join(", ")
            )
        })
        .collect();
    let pack = crate::context::assemble(
        store,
        id,
        &crate::context::Focus { parents: 1, mentions: 1, requirements: 2 },
        16_000,
    )
    .map(|p| p.pack)
    .unwrap_or_default();
    let generated: Vec<String> = state.map.keys().cloned().collect();
    Ok(json!({
        "entity": id,
        "name": e.name,
        "unit": store.out.join("codegen").join(format!("{}.{}", slug, ext_for(lang))).to_string_lossy(),
        "instructions": instructions(lang),
        "context": pack,
        "relationships": rels,
        "requirementGroups": groups,
        "changed": changed,
        "generatedUnits": generated,
    }))
}

// Record an entity's current facts as generated.
pub fn mark(store: &Store, id: &str) -> Result<Value, String> {
    if !store.graph.entities.contains_key(id) {
        return Err(format!("unknown entity `{}`", id));
    }
    let slug = slug_of(id);
    let mut state = GenState::load(&store.out);
    state.mark(&slug, fact_hash(store, id), reqs_of_sorted(store, id));
    state.save();
    Ok(json!({"marked": id}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn fixture(out: &std::path::Path) -> Store {
        let mut s = Store { out: out.to_path_buf(), ..Default::default() };
        s.graph.entities.insert("ent:cart".into(), Entity { name: "Cart".into(), ..Default::default() });
        s.graph.requirements.insert(
            "req:shop-1".into(),
            Requirement {
                ears: "The Cart shall hold items.".into(),
                entities: vec!["ent:cart".into()],
                edges: vec![],
                source: SourceRef { doc: "shop.md".into(), section: "/shop".into(), quote: "holds".into() },
                confidence: None,
                reasoning: None,
                created: None,
                updated: None,
            },
        );
        s
    }

    #[test]
    fn pending_diff_and_mark_lifecycle() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-test-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let s = fixture(&out);
        let p = pending(&s, "rust");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["reason"], "new");
        assert_eq!(p[0]["changed"][0], "req:shop-1 (added)");

        // Mark plus an existing unit file makes it disappear from pending.
        std::fs::create_dir_all(out.join("codegen")).ok();
        std::fs::write(out.join("codegen/cart.rs"), "// unit").ok();
        mark(&s, "ent:cart").unwrap();
        assert!(pending(&s, "rust").is_empty());

        // A new requirement reappears as a precise diff.
        let mut s2 = fixture(&out);
        s2.graph.requirements.insert(
            "req:shop-2".into(),
            Requirement {
                ears: "The Cart shall empty on checkout.".into(),
                entities: vec!["ent:cart".into()],
                edges: vec![],
                source: SourceRef { doc: "shop.md".into(), section: "/shop".into(), quote: "empty".into() },
                confidence: None,
                reasoning: None,
                created: None,
                updated: None,
            },
        );
        let p2 = pending(&s2, "rust");
        assert_eq!(p2.len(), 1);
        assert_eq!(p2[0]["reason"], "changed");
        assert_eq!(p2[0]["changed"][0], "req:shop-2 (added)");

        let pkg = task_package(&s2, "ent:cart", "rust").unwrap();
        assert!(pkg["instructions"].as_str().unwrap().contains("traceability"));
        assert_eq!(pkg["requirementGroups"][0].as_array().unwrap().len(), 2);
    }
}
