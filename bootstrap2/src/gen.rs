// Generation: the shared contract between every generation worker, the built-in
// `jazyk gen` and external MCP agents alike. One task per entity produces the entity's
// part of the deliverable and the tests for its requirements; the ledger binds them to
// the graph. Mirrors docs2/consumers/gen.md and docs2/compiler/tools.md#generation-tools.
use crate::model::hash_hex;
use crate::store::Store;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

// Resolved [gen] settings: where the deliverable lives and the lang hint.
// Mirrors docs2/compiler/project-settings.md#generation.
#[derive(Clone)]
pub struct GenSettings {
    pub deliverable: PathBuf,
    pub lang: String,
}

impl GenSettings {
    pub fn resolve(proj: &crate::project::Project, out: &Path) -> GenSettings {
        let deliverable = match &proj.gen_deliverable {
            Some(d) => proj.root.join(d),
            None => out.join("gen").join("deliverable"),
        };
        GenSettings { deliverable, lang: proj.gen_lang.clone() }
    }

    pub fn from_out(out: &Path) -> GenSettings {
        GenSettings { deliverable: out.join("gen").join("deliverable"), lang: "rust".into() }
    }
}

// The ledger: gen/ledger.yaml. Two maps: generation state per entity, verification
// state per requirement. Mirrors docs2/consumers/gen.md#the-ledger.
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Ledger {
    #[serde(default)]
    pub entities: BTreeMap<String, EntityGen>,
    #[serde(default)]
    pub requirements: BTreeMap<String, ReqRow>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EntityGen {
    pub fact_hash: String,
    #[serde(default)]
    pub requirements: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReqRow {
    pub entity: String,
    #[serde(default)]
    pub files: Vec<String>,
    pub test: TestRef,
    pub hashes: RowHashes,
    #[serde(default = "verdict_none")]
    pub verdict: String, // none | pass | fail
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

fn verdict_none() -> String {
    "none".into()
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TestRef {
    pub kind: String,  // programmatic | llm
    pub label: String, // freeform, the generator's own words
    pub artifact: String,
    pub name: String,
    pub run: String,
    #[serde(default = "dot")]
    pub cwd: String,
}

fn dot() -> String {
    ".".into()
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct RowHashes {
    pub requirement: String,
    pub test: String,
    pub files: String,
}

impl Ledger {
    pub fn path(out: &Path) -> PathBuf {
        out.join("gen").join("ledger.yaml")
    }

    pub fn load(out: &Path) -> Ledger {
        std::fs::read_to_string(Self::path(out))
            .ok()
            .and_then(|t| serde_norway::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, out: &Path) {
        let path = Self::path(out);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Ok(text) = serde_norway::to_string(self) {
            std::fs::write(&path, text).ok();
        }
    }
}

// Where a test artifact lives on disk: llm criteria are metadata (under <out>/gen/),
// programmatic artifacts are part of the deliverable.
pub fn artifact_path(out: &Path, gs: &GenSettings, test: &TestRef) -> PathBuf {
    if test.kind == "llm" {
        out.join("gen").join(&test.artifact)
    } else {
        gs.deliverable.join(&test.artifact)
    }
}

pub fn hash_file(path: &Path) -> String {
    std::fs::read(path).map(|b| hash_hex(&String::from_utf8_lossy(&b))).unwrap_or_default()
}

// Hash over a row's manifest files, sorted, concatenated. Deliverable-relative paths.
pub fn hash_files(gs: &GenSettings, files: &[String]) -> String {
    let mut sorted: Vec<&String> = files.iter().collect();
    sorted.sort();
    let mut acc = String::new();
    for f in sorted {
        acc.push_str(f);
        acc.push('|');
        acc.push_str(&hash_file(&gs.deliverable.join(f)));
        acc.push('|');
    }
    hash_hex(&acc)
}

pub fn slug_of(id: &str) -> String {
    id.strip_prefix("ent:").unwrap_or(id).to_string()
}

pub fn req_slug(id: &str) -> String {
    id.strip_prefix("req:").unwrap_or(id).to_string()
}

// The suggested test name: requirement id + hash prefix, sanitized. A reworded
// requirement mechanically breaks the recorded run filter.
pub fn test_name(rid: &str, ears: &str) -> String {
    let sanitized: String = req_slug(rid)
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    format!("req_{}_{}", sanitized, &hash_hex(ears)[..8])
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

// The generation contract, identical for every worker.
pub fn instructions(lang: &str) -> String {
    format!(
        "Generate the entity's part of the deliverable AND the tests for its requirements, in {lang}.\n\
         - The task package names the deliverable directory and suggests a default layout; you may lay out files differently, but every file you write must appear in the manifest you pass to gen_mark.\n\
         - Every requirement is an obligation; implement each and place a marker comment at the implementing site: the requirement id, hash, and the verbatim quote.\n\
         - Derive one test per requirement. Pick the kind per requirement:\n\
           - programmatic: any test a command can run (unit, integration, cucumber are examples, not a taxonomy). Write the test into the deliverable and record the exact command that runs only that test. Its exit code is the verdict.\n\
           - llm: the requirement needs judgment, or the deliverable is not executable software. Write a criteria file (the package names its path): front matter with the requirement id and statement hash, then the statement, the quote, the implementing file paths, the steps to confirm, and the verdict contract (PASS or FAIL plus reasoning).\n\
         - Name each test with the suggested testName from the package (requirement id plus hash prefix) and put the marker comment above it.\n\
         - Reference other entities' files through the manifest the package carries.\n\
         - Dense entities generate in parts of {GROUP} requirements: part 1 is the types, state, and the first group; each later part receives what exists so far and returns ONLY additional content to append.\n\
         - Return only file content, never fences or prose, when asked for a file."
    )
}

// The change diff for one entity versus the ledger.
fn change_diff(ledger: &Ledger, slug: &str, current: &[String]) -> (String, Vec<String>) {
    match ledger.entities.get(slug) {
        None => ("new".to_string(), current.iter().map(|r| format!("{} (added)", r)).collect()),
        Some(e) => {
            let mut changed: Vec<String> = Vec::new();
            for r in current {
                if !e.requirements.contains(r) {
                    changed.push(format!("{} (added)", r));
                }
            }
            for r in &e.requirements {
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

// Entities whose facts differ from the ledger, or whose recorded files are missing.
pub fn pending(store: &Store, gs: &GenSettings) -> Vec<Value> {
    let ledger = Ledger::load(&store.out);
    let mut out = Vec::new();
    for id in store.graph.entities.keys() {
        let rids = reqs_of_sorted(store, id);
        if rids.is_empty() {
            continue;
        }
        let slug = slug_of(id);
        let hash = fact_hash(store, id);
        let current = ledger.entities.get(&slug).map(|e| {
            e.fact_hash == hash && !e.files.is_empty() && e.files.iter().all(|f| gs.deliverable.join(f).exists())
        });
        if current == Some(true) {
            continue;
        }
        let (reason, changed) = change_diff(&ledger, &slug, &rids);
        out.push(json!({
            "entity": id,
            "reason": reason,
            "changed": changed,
        }));
    }
    out
}

// The full package a worker needs for one task.
pub fn task_package(store: &Store, id: &str, gs: &GenSettings) -> Result<Value, String> {
    if !store.graph.entities.contains_key(id) {
        return Err(format!("unknown entity `{}`", id));
    }
    let ledger = Ledger::load(&store.out);
    let e = &store.graph.entities[id];
    let slug = slug_of(id);
    let stem = slug.replace('-', "_");
    let ext = ext_for(&gs.lang);
    let rids = reqs_of_sorted(store, id);
    let (_, changed) = change_diff(&ledger, &slug, &rids);
    let groups: Vec<Vec<Value>> = rids
        .chunks(GROUP)
        .map(|chunk| {
            chunk
                .iter()
                .filter_map(|rid| {
                    store.graph.requirements.get(rid).map(|r| {
                        json!({
                            "id": rid,
                            "ears": r.ears,
                            "quote": r.source.quote,
                            "hash": hash_hex(&r.ears),
                            "testName": test_name(rid, &r.ears),
                            "criteriaPath": format!("criteria/req-{}.md", req_slug(rid)),
                        })
                    })
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
    let manifest: BTreeMap<&String, &Vec<String>> =
        ledger.entities.iter().map(|(k, v)| (k, &v.files)).collect();
    Ok(json!({
        "entity": id,
        "name": e.name,
        "deliverable": gs.deliverable.to_string_lossy(),
        "suggestedLayout": {
            "product": format!("src/{}.{}", stem, ext),
            "tests": format!("tests/{}.{}", stem, ext),
        },
        "factHash": fact_hash(store, id),
        "lang": gs.lang,
        "instructions": instructions(&gs.lang),
        "context": pack,
        "relationships": rels,
        "requirementGroups": groups,
        "changed": changed,
        "generatedFiles": manifest,
    }))
}

// Record a task done. The manifest binds the worker's files to the graph and seeds the
// verification rows. Mirrors docs2/compiler/tools.md#generation-tools (gen_mark).
pub fn mark(store: &Store, id: &str, fact_hash_seen: Option<&str>, manifest: &Value, gs: &GenSettings) -> Result<Value, String> {
    if !store.graph.entities.contains_key(id) {
        return Err(format!("unknown entity `{}`", id));
    }
    let slug = slug_of(id);
    let files: Vec<String> = manifest["files"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let mut ledger = Ledger::load(&store.out);
    ledger.entities.insert(
        slug,
        EntityGen {
            fact_hash: fact_hash_seen.map(String::from).unwrap_or_else(|| fact_hash(store, id)),
            requirements: reqs_of_sorted(store, id),
            files: files.clone(),
        },
    );
    let mut seeded = 0;
    if let Some(tests) = manifest["tests"].as_array() {
        for t in tests {
            let Some(rid) = t["requirement"].as_str() else { continue };
            let rid = store.resolve_id(rid).to_string();
            let Some(r) = store.graph.requirements.get(&rid) else {
                return Err(format!("unknown requirement `{}` in manifest", rid));
            };
            let test = TestRef {
                kind: t["kind"].as_str().unwrap_or("programmatic").to_string(),
                label: t["label"].as_str().unwrap_or("test").to_string(),
                artifact: t["artifact"].as_str().unwrap_or_default().to_string(),
                name: t["name"].as_str().unwrap_or(&test_name(&rid, &r.ears)).to_string(),
                run: t["run"].as_str().unwrap_or_default().to_string(),
                cwd: t["cwd"].as_str().unwrap_or(".").to_string(),
            };
            let row_files: Vec<String> = t["files"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .filter(|v: &Vec<String>| !v.is_empty())
                .unwrap_or_else(|| files.clone());
            let hashes = RowHashes {
                requirement: hash_hex(&r.ears),
                test: hash_file(&artifact_path(&store.out, gs, &test)),
                files: hash_files(gs, &row_files),
            };
            let owner = r
                .entities
                .first()
                .map(|e| store.resolve_id(e).to_string())
                .unwrap_or_else(|| id.to_string());
            ledger.requirements.insert(
                rid.clone(),
                ReqRow {
                    entity: owner,
                    files: row_files,
                    test,
                    hashes,
                    verdict: "none".into(),
                    last_run: None,
                    evidence: None,
                },
            );
            seeded += 1;
        }
    }
    ledger.save(&store.out);
    Ok(json!({"marked": id, "files": files.len(), "tests": seeded}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn fixture(out: &std::path::Path) -> (Store, GenSettings) {
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
        let gs = GenSettings { deliverable: out.join("product"), lang: "rust".into() };
        (s, gs)
    }

    #[test]
    fn pending_diff_and_mark_lifecycle() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-test-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        let p = pending(&s, &gs);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["reason"], "new");
        assert_eq!(p[0]["changed"][0], "req:shop-1 (added)");

        // A mark with a manifest whose files exist makes it disappear from pending and
        // seeds a verification row.
        std::fs::create_dir_all(gs.deliverable.join("src")).ok();
        std::fs::create_dir_all(gs.deliverable.join("tests")).ok();
        std::fs::write(gs.deliverable.join("src/cart.rs"), "// product").ok();
        std::fs::write(gs.deliverable.join("tests/cart.rs"), "// req:shop-1\nfn t() {}").ok();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        let manifest = serde_json::json!({
            "files": ["src/cart.rs", "tests/cart.rs"],
            "tests": [{
                "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
                "artifact": "tests/cart.rs", "name": name,
                "run": format!("cargo test {}", name), "files": ["src/cart.rs"],
            }],
        });
        let r = mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
        assert_eq!(r["tests"], 1);
        assert!(pending(&s, &gs).is_empty());
        let ledger = Ledger::load(&out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(row.verdict, "none");
        assert_eq!(row.hashes.requirement, hash_hex("The Cart shall hold items."));

        // A new requirement reappears as a precise diff.
        let mut s2 = s.clone();
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
        let p2 = pending(&s2, &gs);
        assert_eq!(p2.len(), 1);
        assert_eq!(p2[0]["reason"], "changed");
        assert_eq!(p2[0]["changed"][0], "req:shop-2 (added)");

        let pkg = task_package(&s2, "ent:cart", &gs).unwrap();
        assert!(pkg["instructions"].as_str().unwrap().contains("manifest"));
        let g0 = pkg["requirementGroups"][0].as_array().unwrap();
        assert_eq!(g0.len(), 2);
        assert!(g0[0]["testName"].as_str().unwrap().starts_with("req_shop_1_"));
    }
}
