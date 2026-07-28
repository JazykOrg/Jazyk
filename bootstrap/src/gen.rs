// Generation: the shared contract between every generation worker, the built-in
// `jazyk gen` and external MCP agents alike. One task per entity produces the entity's
// part of the deliverable and the tests for its requirements; the ledger binds them to
// the graph. Mirrors docs/consumers/gen.md and docs/compiler/tools.md#generation-tools.
use crate::model::hash_hex;
use crate::store::Store;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// Requirements per generation part for dense entities.
pub const GROUP: usize = 20;

// Resolved [gen] settings: where the deliverable lives. Nothing else: what the
// deliverable is comes from the documents through the graph, never from configuration.
// Mirrors docs/compiler/project-settings.md#generation.
#[derive(Clone)]
pub struct GenSettings {
    pub deliverable: PathBuf,
}

impl GenSettings {
    pub fn resolve(proj: &crate::project::Project) -> GenSettings {
        let deliverable = match &proj.gen_deliverable {
            Some(d) => proj.root.join(d),
            // Default: the project root itself. The docs glob keeps the source tree
            // out of the product's way (docs/compiler/project-settings.md#generation).
            None => proj.root.clone(),
        };
        GenSettings { deliverable }
    }

    // Placeholder for sessions with no project (benchmark cases). Gen tools are absent
    // from those toolsets, so the path is never read.
    pub fn from_out(out: &Path) -> GenSettings {
        GenSettings { deliverable: out.join("gen").join("deliverable") }
    }
}

// The ledger: gen/ledger.yaml. Two maps: generation state per entity, verification
// state per requirement. Mirrors docs/consumers/gen.md#the-ledger.
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
    // Anchored implementing sites, recorded from stripped markers at mark time.
    // Mirrors docs/consumers/gen.md#traceability.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sites: Vec<Site>,
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

// One anchored implementing site: the file, the 1-based line in the stripped file,
// and the verbatim next significant line the marker sat above. Language-agnostic:
// relocation is a whitespace-insensitive line match, never a parse of the medium.
#[derive(Serialize, Deserialize, Clone)]
pub struct Site {
    pub file: String,
    pub line: usize,
    pub head: String,
}

// A site freshly stripped from a file, before the requirement id is resolved.
pub struct RawSite {
    pub rid: String,
    pub line: usize,
    pub head: String,
}

// Strip single-line markers (`req:<slug> hash:<hex>` with nothing but comment leaders
// around it) and return the clean text plus the sites they anchored. A marker line
// carrying other alphanumeric content is left alone: the harness never mangles code.
// Mirrors docs/consumers/gen.md#traceability.
pub fn strip_markers(text: &str) -> (String, Vec<RawSite>) {
    let re = regex::Regex::new(r"req:([A-Za-z0-9][A-Za-z0-9_-]*)\s+hash:([0-9a-fA-F]{4,64})").unwrap();
    let mut clean: Vec<&str> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    let mut sites: Vec<RawSite> = Vec::new();
    for line in text.lines() {
        if let Some(c) = re.captures(line) {
            let rest = line.replacen(c.get(0).unwrap().as_str(), "", 1);
            if !rest.chars().any(|ch| ch.is_alphanumeric()) {
                pending.push(format!("req:{}", &c[1]));
                continue;
            }
        }
        clean.push(line);
        if !pending.is_empty() && !line.trim().is_empty() {
            let lineno = clean.len();
            for rid in pending.drain(..) {
                sites.push(RawSite { rid, line: lineno, head: line.to_string() });
            }
        }
    }
    let mut out = clean.join("\n");
    if text.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    (out, sites)
}

// Locate a site's head in the current text: whitespace-insensitive line match, the
// occurrence nearest the recorded line wins. Returns (1-based line, still-at-hint).
// No match means the site is lost; the caller shows it as such, never guesses.
pub fn locate_head(text: &str, head: &str, hint: usize) -> Option<(usize, bool)> {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let h = norm(head);
    if h.is_empty() {
        return None;
    }
    let mut best: Option<usize> = None;
    for (i, line) in text.lines().enumerate() {
        if norm(line) == h {
            let ln = i + 1;
            let closer = best
                .map(|b| (ln as i64 - hint as i64).abs() < (b as i64 - hint as i64).abs())
                .unwrap_or(true);
            if closer {
                best = Some(ln);
            }
        }
    }
    best.map(|ln| (ln, ln == hint))
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

fn dedup_keep_order(files: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    files.into_iter().filter(|f| seen.insert(f.clone())).collect()
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

// The generation contract, identical for every worker. It never names a medium: what
// the deliverable is (a language, a format, a genre) is a fact the documents state,
// carried by the context pack.
pub fn instructions() -> String {
    format!(
        "Generate the entity's part of the deliverable AND the tests for its requirements.\n\
         - Derive the medium from the requirements and the context: what the documents say the deliverable is decides what you write. You choose the layout, the file names, and the build or support files that make your recorded commands executable; every file you write must appear in the manifest you pass to gen_mark.\n\
         - One toolchain per deliverable: the run commands already recorded (the package lists them) establish the language, the test runner, and the command style; reuse them exactly. Never introduce a second test runner.\n\
         - Every deliverable file belongs to one entity. Never write to a file another entity's task produced (the package lists them); reference it through imports instead and pick a path of your own.\n\
         - Each recorded run command must execute from the deliverable directory as recorded. If it needs a build or configuration file that no task has written yet (package.json, Cargo.toml), return that file as a support file in the manifest step.\n\
         - Every requirement is an obligation; implement each and place a single-line marker comment directly above the implementing site: `req:<id> hash:<hash8>` in the medium's comment syntax, nothing else on that line. The harness strips marker lines from your files and records each location in the ledger, so the delivered file stays clean.\n\
         - Derive one test per requirement. Pick the kind per requirement:\n\
           - programmatic: any test a command can run (unit, integration, cucumber are examples, not a taxonomy). Write the test into the deliverable and record the exact command that runs only that test, exactly as it must be executed from the deliverable directory. Its exit code is the verdict, so the artifact must propagate failure: a harness that prints a failure and still exits zero verifies nothing.\n\
           - llm: the requirement needs judgment, or the deliverable is not executable software. Write a criteria file (the package names its path): front matter with the requirement id and statement hash, then the statement, the quote, the implementing file paths, the steps to confirm, and the verdict contract (PASS or FAIL plus reasoning).\n\
         - Name each test with the suggested testName from the package (requirement id plus hash prefix) and put the single-line marker comment above it.\n\
         - Reference other entities' files through the manifest the package carries.\n\
         - Dense entities generate in parts of {GROUP} requirements: part 1 is the core plus the first group; each later part receives what exists so far and returns ONLY additional content to append.\n\
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
    // The established conventions: run commands other tasks already recorded. One
    // toolchain per deliverable (docs/consumers/gen.md#file-ownership-and-conventions).
    let mut run_commands: Vec<String> = ledger
        .requirements
        .values()
        .filter(|r| r.test.kind == "programmatic" && !r.test.run.trim().is_empty())
        .map(|r| r.test.run.clone())
        .collect();
    run_commands.sort();
    run_commands.dedup();
    Ok(json!({
        "entity": id,
        "name": e.name,
        "deliverable": gs.deliverable.to_string_lossy(),
        "factHash": fact_hash(store, id),
        "instructions": instructions(),
        "context": pack,
        "relationships": rels,
        "requirementGroups": groups,
        "changed": changed,
        "generatedFiles": manifest,
        "runCommands": run_commands,
    }))
}

// Record a task done. The manifest binds the worker's files to the graph and seeds the
// verification rows. Mirrors docs/compiler/tools.md#generation-tools (gen_mark).
pub fn mark(store: &Store, id: &str, fact_hash_seen: Option<&str>, manifest: &Value, gs: &GenSettings) -> Result<Value, String> {
    if !store.graph.entities.contains_key(id) {
        return Err(format!("unknown entity `{}`", id));
    }
    let slug = slug_of(id);
    // File lists are sets: dedup on write, preserving first-seen order.
    let files: Vec<String> = dedup_keep_order(
        manifest["files"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default(),
    );
    // Strip marker lines from the manifest files and collect the sites they anchor.
    // The marker is a wire format: the worker localizes while writing, the harness
    // records and cleans. Runs before hashing so every hash sees the stripped bytes.
    // Mirrors docs/consumers/gen.md#traceability.
    let mut sites_by_rid: BTreeMap<String, Vec<Site>> = BTreeMap::new();
    for f in &files {
        let path = gs.deliverable.join(f);
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let (clean, raw) = strip_markers(&text);
        if raw.is_empty() {
            continue;
        }
        std::fs::write(&path, clean).map_err(|e| e.to_string())?;
        for s in raw {
            let rid = store.resolve_id(&s.rid).to_string();
            sites_by_rid.entry(rid).or_default().push(Site { file: f.clone(), line: s.line, head: s.head });
        }
    }
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
            let row_files: Vec<String> = dedup_keep_order(
                t["files"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .filter(|v: &Vec<String>| !v.is_empty())
                    .unwrap_or_else(|| files.clone()),
            );
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
                    sites: sites_by_rid.remove(&rid).unwrap_or_default(),
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
        let gs = GenSettings { deliverable: out.join("product") };
        (s, gs)
    }

    #[test]
    fn strip_markers_and_relocate() {
        let text = "// intro\n// req:main-js-5 hash:310b7c2e\n\nfunction trim(line) {\n  return line.trim()\n}\n";
        let (clean, sites) = strip_markers(text);
        assert!(!clean.contains("hash:"), "{}", clean);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].rid, "req:main-js-5");
        // The head is the next significant line, past the blank one.
        assert_eq!(sites[0].head, "function trim(line) {");
        assert_eq!(sites[0].line, 3);

        // An inline marker beside code is never stripped: the harness must not mangle.
        let (kept, none) = strip_markers("let x = 1 // req:a-1 hash:deadbeef\n");
        assert_eq!(kept, "let x = 1 // req:a-1 hash:deadbeef\n");
        assert!(none.is_empty());

        // Relocation: whitespace-insensitive, nearest to the hint, lost when gone.
        let edited = "// new header\n\nfunction trim(line)   {\n  return line.trim()\n}\n";
        let (line, exact) = locate_head(edited, &sites[0].head, sites[0].line).unwrap();
        assert_eq!(line, 3);
        assert!(exact);
        let moved = "a\nb\nc\nd\nfunction trim(line) {\n";
        assert_eq!(locate_head(moved, &sites[0].head, 3), Some((5, false)));
        assert_eq!(locate_head("nothing here\n", &sites[0].head, 3), None);
    }

    #[test]
    fn mark_strips_markers_into_sites() {
        let out = std::env::temp_dir().join(format!("jazyk-sites-test-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        std::fs::create_dir_all(gs.deliverable.join("src")).ok();
        std::fs::create_dir_all(gs.deliverable.join("tests")).ok();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        std::fs::write(
            gs.deliverable.join("src/cart.rs"),
            "// req:shop-1 hash:12345678\nfn hold(i: Item) {}\n",
        )
        .ok();
        std::fs::write(
            gs.deliverable.join("tests/cart.rs"),
            format!("// req:shop-1 hash:12345678\nfn {}() {{}}\n", name),
        )
        .ok();
        let manifest = serde_json::json!({
            "files": ["src/cart.rs", "tests/cart.rs"],
            "tests": [{
                "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
                "artifact": "tests/cart.rs", "name": name,
                "run": format!("cargo test {}", name), "files": ["src/cart.rs"],
            }],
        });
        mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
        // The written files are clean; the ledger row carries the anchored sites.
        let product = std::fs::read_to_string(gs.deliverable.join("src/cart.rs")).unwrap();
        assert_eq!(product, "fn hold(i: Item) {}\n");
        let ledger = Ledger::load(&out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(row.sites.len(), 2);
        assert_eq!(row.sites[0].file, "src/cart.rs");
        assert_eq!(row.sites[0].line, 1);
        assert_eq!(row.sites[0].head, "fn hold(i: Item) {}");
        // Hashes were computed over the stripped bytes: the row is not stale-code.
        let (status, _) = crate::verify::status_of(&s, "req:shop-1", row, &gs);
        assert_eq!(status, "unverified");
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

// ---- the built-in worker ----
// Moved out of the CLI so the GUI job runner drives the same loop. The CLI wrapper
// renders the worker events on the historical output format; the GUI streams them.

// Run generation over the given entities (all entities with requirements when empty),
// leaf-first over the relationship graph, skipping entities whose facts are unchanged
// unless forced. Returns {regenerated, skipped, failures}.
pub fn run_all(
    store: &Store,
    llm: &crate::llm::Llm,
    gs: &GenSettings,
    entities: &[String],
    force: bool,
    trace: &crate::turn::Trace,
) -> Result<Value, String> {
    use crate::turn::TraceEvent;
    let mut targets: Vec<String> = if entities.is_empty() {
        store
            .graph
            .entities
            .keys()
            .filter(|id| !store.requirements_referencing(id).is_empty())
            .cloned()
            .collect()
    } else {
        let mut v = Vec::new();
        for e in entities {
            let id = store.resolve_id(e).to_string();
            if store.graph.entities.contains_key(&id) {
                v.push(id);
            } else {
                return Err(format!("unknown entity `{}`", e));
            }
        }
        v
    };
    if targets.is_empty() {
        return Err("no entities with requirements; run `jazyk compile` first".into());
    }

    // Leaf-first ordering: repeatedly emit the entity with the fewest ungenerated
    // neighbors over the relationship graph (ties by name).
    let neighbors = |id: &str| -> Vec<String> {
        store
            .graph
            .relationships
            .values()
            .filter(|r| r.members.iter().any(|m| m == id))
            .flat_map(|r| r.members.iter().filter(|m| *m != id).cloned())
            .collect()
    };
    let mut ordered: Vec<String> = Vec::new();
    while !targets.is_empty() {
        let (i, _) = targets
            .iter()
            .enumerate()
            .min_by_key(|(_, id)| {
                let pending = neighbors(id).iter().filter(|n| targets.contains(n)).count();
                (pending, (*id).clone())
            })
            .unwrap();
        ordered.push(targets.remove(i));
    }

    std::fs::create_dir_all(&gs.deliverable).ok();
    let pending_set: std::collections::BTreeSet<String> =
        pending(store, gs).iter().filter_map(|p| p["entity"].as_str().map(String::from)).collect();
    let (mut regenerated, mut skipped, mut failures) = (0u64, 0u64, 0u64);
    for id in &ordered {
        if trace.is_cancelled() {
            break;
        }
        if !force && !pending_set.contains(id) {
            skipped += 1;
            trace.event(TraceEvent::GenEntitySkipped { entity: id.clone(), reason: "unchanged".into() });
            continue;
        }
        trace.event(TraceEvent::GenEntityStart { entity: id.clone() });
        let task = match task_package(store, id, gs) {
            Ok(t) => t,
            Err(e) => {
                trace.event(TraceEvent::GenEntityFailed { entity: id.clone(), stage: "task".into(), error: e });
                failures += 1;
                continue;
            }
        };
        match gen_one(store, llm, gs, id, &task) {
            Ok(files) => {
                trace.event(TraceEvent::GenEntityDone { entity: id.clone(), files });
                regenerated += 1;
            }
            Err(e) => {
                trace.event(TraceEvent::GenEntityFailed { entity: id.clone(), stage: "generate".into(), error: e });
                failures += 1;
            }
        }
    }
    Ok(json!({ "regenerated": regenerated, "skipped": skipped, "failures": failures }))
}

pub fn parse_file_reply(reply: &str) -> Result<(String, String), String> {
    let reply = strip_fences(reply);
    let mut lines = reply.splitn(2, '\n');
    let first = lines.next().unwrap_or("").trim();
    let Some(path) = first.strip_prefix("FILE:") else {
        return Err(format!("first line must be `FILE: <path>`, got `{}`", crate::llm::truncate(first, 80)));
    };
    let path = path.trim();
    if path.is_empty() || path.starts_with('/') || path.contains("..") {
        return Err(format!("bad file path `{}`", path));
    }
    Ok((path.to_string(), lines.next().unwrap_or("").to_string()))
}

// Models wrap code in markdown fences despite instructions; strip one outer fence.
pub fn strip_fences(s: &str) -> String {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        if let Some(end) = rest.rfind("```") {
            let body = &rest[..end];
            return body.split_once('\n').map(|(_, b)| b).unwrap_or(body).to_string();
        }
    }
    t.to_string()
}

// One entity's task: the model picks the product path (FILE protocol, parts when
// dense), the tests path, and the manifest with the run commands. Requirements the
// model declares untestable programmatically, or whose declared test fails validation
// (name missing from the artifact, empty command), become llm rows with a criteria
// file. Manifest validation is deterministic; nothing here chooses for the model.
// Snapshot the previous content of a deliverable file before this run rewrites or
// removes it: the diff baseline frontends show against. A file the run creates fresh
// has no baseline, so a stale snapshot from an earlier run is dropped. File ownership
// keeps a path with one entity, so a per-gen_one set is once per run per file.
// Mirrors docs/consumers/gen.md#incremental-regeneration.
fn snapshot_baseline(out: &Path, gs: &GenSettings, rel: &str, seen: &mut std::collections::HashSet<String>) {
    if !seen.insert(rel.to_string()) {
        return;
    }
    let dst = out.join("deliverable-baseline").join(rel);
    match std::fs::read(gs.deliverable.join(rel)) {
        Ok(bytes) => {
            if let Some(p) = dst.parent() {
                std::fs::create_dir_all(p).ok();
            }
            std::fs::write(&dst, bytes).ok();
        }
        Err(_) => {
            std::fs::remove_file(&dst).ok();
        }
    }
}

pub fn gen_one(store: &Store, llm: &crate::llm::Llm, gs: &GenSettings, id: &str, task: &serde_json::Value) -> Result<usize, String> {
    let mut baselined: std::collections::HashSet<String> = Default::default();
    let instructions = task["instructions"].as_str().unwrap_or_default();
    let run_commands = task["runCommands"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    let header = format!(
        "Entity {} ({})\nContext:\n{}\nChanged since last generation: {}\nAlready generated files (each belongs to its entity; never write to another entity's file): {}\nRecorded run commands (the established toolchain; reuse it): {}\n",
        id,
        task["name"].as_str().unwrap_or_default(),
        task["context"].as_str().unwrap_or_default(),
        task["changed"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
        serde_json::to_string(&task["generatedFiles"]).unwrap_or_default(),
        if run_commands.is_empty() { "(none yet; this task establishes them)" } else { &run_commands },
    );
    // File ownership: a path recorded for a different entity is off limits. One
    // corrective retry, then the task fails.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    let ledger = Ledger::load(&store.out);
    let own_slug = slug_of(id);
    let owner_of = |path: &str| -> Option<String> {
        ledger
            .entities
            .iter()
            .find(|(slug, e)| slug.as_str() != own_slug && e.files.iter().any(|f| f == path))
            .map(|(slug, _)| slug.clone())
    };
    let groups = task["requirementGroups"].as_array().cloned().unwrap_or_default();
    let parts = groups.len();
    let req_line = |r: &serde_json::Value| {
        format!(
            "- {} [{}]: {}\n  quote: {}",
            r["id"].as_str().unwrap_or(""),
            r["testName"].as_str().unwrap_or(""),
            r["ears"].as_str().unwrap_or(""),
            r["quote"].as_str().unwrap_or("")
        )
    };

    // Product content; the model names the file.
    let mut code = String::new();
    let mut product_rel = String::new();
    for (k, group) in groups.iter().enumerate() {
        let req_lines: Vec<String> = group.as_array().map(|a| a.iter().map(req_line).collect()).unwrap_or_default();
        let user = if k == 0 {
            format!(
                "{}\nWrite the implementing content for this entity. Derive the medium from the context; choose the file path yourself, relative to the deliverable. Reply with the first line exactly `FILE: <path>` and the file content after it. Put a single-line marker comment (req:<id> hash:<hash8> in the medium's comment syntax, alone on its line) directly above each implementing site; the harness strips it and records the location. Requirements (group 1 of {}):\n{}\n",
                header, parts, req_lines.join("\n")
            )
        } else {
            format!(
                "{}\nRequirements (group {} of {}):\n{}\n\nThe file `{}` so far:\n{}\nReturn ONLY additional content to append, no FILE line.",
                header, k + 1, parts, req_lines.join("\n"), product_rel, crate::llm::truncate(&code, 20_000)
            )
        };
        let reply = llm
            .chat(instructions, &user, &format!("gen {} product {}/{}", id, k + 1, parts))
            .map_err(|e| format!("product part {}/{}: {}", k + 1, parts, e))?;
        if k == 0 {
            let (mut path, mut body) = parse_file_reply(&reply)?;
            if let Some(owner) = owner_of(&path) {
                let retry = format!(
                    "{}\nThe path `{}` already belongs to entity `{}`; never write to another entity's file. Reply again, same content, under a file path of your own: first line exactly `FILE: <path>`, content after it.",
                    user, path, owner
                );
                let reply2 = llm
                    .chat(instructions, &retry, &format!("gen {} product retry", id))
                    .map_err(|e| format!("product retry: {}", e))?;
                let (p, b) = parse_file_reply(&reply2)?;
                if let Some(o) = owner_of(&p) {
                    return Err(format!("product path `{}` belongs to entity `{}`", p, o));
                }
                path = p;
                body = b;
            }
            product_rel = path;
            code = body;
        } else {
            code.push_str("\n");
            code.push_str(&strip_fences(&reply));
        }
        code.push('\n');
    }
    if code.trim().is_empty() {
        return Err("empty product".into());
    }
    let product_path = gs.deliverable.join(&product_rel);
    if let Some(p) = product_path.parent() {
        std::fs::create_dir_all(p).ok();
    }
    snapshot_baseline(&store.out, gs, &product_rel, &mut baselined);
    std::fs::write(&product_path, &code).map_err(|e| e.to_string())?;

    // Tests: the model names the file too, or declares nothing programmatic.
    let all_reqs: Vec<serde_json::Value> = groups.iter().flat_map(|g| g.as_array().cloned().unwrap_or_default()).collect();
    let req_lines: Vec<String> = all_reqs.iter().map(req_line).collect();
    let tests_user = format!(
        "{}\nWrite the tests for the requirements against `{}` (content below). Choose the test file path yourself. One test per requirement you can test programmatically, named EXACTLY by its [testName], with the single-line marker comment above it. Reply with the first line exactly `FILE: <path>` and the content after it. If no requirement can be tested programmatically, reply with exactly `NONE`. Requirements:\n{}\n\nThe product file:\n{}\n",
        header,
        product_rel,
        req_lines.join("\n"),
        crate::llm::truncate(&code, 16_000)
    );
    let tests_reply = llm
        .chat(instructions, &tests_user, &format!("gen {} tests", id))
        .map_err(|e| format!("tests file: {}", e))?;
    let mut files = vec![product_rel.clone()];
    let mut tests_rel = String::new();
    let mut tests_code = String::new();
    if tests_reply.trim() != "NONE" {
        let (mut path, mut body) = parse_file_reply(&tests_reply).map_err(|e| format!("tests reply: {}", e))?;
        if let Some(owner) = owner_of(&path) {
            let retry = format!(
                "{}\nThe path `{}` already belongs to entity `{}`; never write to another entity's file. Reply again, same content, under a file path of your own: first line exactly `FILE: <path>`, content after it.",
                tests_user, path, owner
            );
            let reply2 = llm
                .chat(instructions, &retry, &format!("gen {} tests retry", id))
                .map_err(|e| format!("tests retry: {}", e))?;
            let (p, b) = parse_file_reply(&reply2).map_err(|e| format!("tests reply: {}", e))?;
            if let Some(o) = owner_of(&p) {
                return Err(format!("tests path `{}` belongs to entity `{}`", p, o));
            }
            path = p;
            body = b;
        }
        tests_rel = path;
        tests_code = body;
        let tests_path = gs.deliverable.join(&tests_rel);
        if let Some(p) = tests_path.parent() {
            std::fs::create_dir_all(p).ok();
        }
        snapshot_baseline(&store.out, gs, &tests_rel, &mut baselined);
        std::fs::write(&tests_path, &tests_code).map_err(|e| e.to_string())?;
        files.push(tests_rel.clone());
    }

    // The manifest: the model declares run commands and any support files it needs;
    // support files are returned inline and written here.
    // Deterministic ground truth for the manifest step: the test names actually
    // present in the artifact. Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    let found_names: Vec<&str> = all_reqs
        .iter()
        .filter_map(|r| r["testName"].as_str())
        .filter(|n| !n.is_empty() && tests_code.contains(*n))
        .collect();
    // A run command that names neither the tests artifact nor the test runs the
    // product, not the test.
    let tests_base = std::path::Path::new(&tests_rel)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let runs_the_test = |run: &str, name: &str| -> bool {
        (!tests_rel.is_empty() && run.contains(&tests_rel))
            || (!tests_base.is_empty() && run.contains(&tests_base))
            || (!name.is_empty() && run.contains(name))
    };
    let manifest_user = format!(
        "Files written so far for entity {}: {:?} under the deliverable directory `{}`.\nRecorded run commands already in use (reuse this toolchain, never introduce a second test runner): {}\nTest names the harness found in the tests file (declare each of these programmatic with its run command; declare a requirement whose name is absent as llm): {}\nEach programmatic run command must invoke the tests artifact and select only that test: the command text must reference the tests file path or the testName. A command that only runs the product is invalid.\nReturn ONLY a JSON object, no prose:\n{{\"supportFiles\": [{{\"path\": \"...\", \"content\": \"...\"}}], \"tests\": [{{\"requirement\": \"req:...\", \"kind\": \"programmatic\"|\"llm\", \"label\": \"your words\", \"name\": \"the testName\", \"run\": \"exact command executed from the deliverable directory that runs only that test\", \"cwd\": \".\"}}]}}\nsupportFiles are build or configuration files required for the run commands to execute (empty array if none are needed or they already exist). A run command that cannot execute from a fresh checkout of the deliverable is a defect: if it needs a runner or build file no listed file provides (a package.json for npx jest, a Cargo.toml for cargo test), you MUST return that file in supportFiles. Every requirement must appear once in tests. Requirements and test names:\n{}\n\nThe tests file `{}`:\n{}\n",
        id,
        files,
        task["deliverable"].as_str().unwrap_or_default(),
        if run_commands.is_empty() { "(none yet; this task establishes the toolchain)" } else { &run_commands },
        if found_names.is_empty() { "(none)".to_string() } else { found_names.join(", ") },
        all_reqs.iter().map(|r| format!("- {} [{}]", r["id"].as_str().unwrap_or(""), r["testName"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("\n"),
        tests_rel,
        crate::llm::truncate(&tests_code, 12_000)
    );
    let manifest_reply = llm
        .chat(instructions, &manifest_user, &format!("gen {} manifest", id))
        .map_err(|e| format!("manifest: {}", e))?;
    let mut manifest_json: serde_json::Value = {
        let text = strip_fences(&manifest_reply);
        let start = text.find('{').ok_or("manifest reply held no JSON object")?;
        let end = text.rfind('}').ok_or("manifest reply held no JSON object")?;
        serde_json::from_str(&text[start..=end]).map_err(|e| format!("manifest JSON: {}", e))?
    };
    // The manifest must agree with the artifact: a present test left undeclared, or a
    // declared programmatic test absent from the artifact, gets one corrective retry.
    // Rows still wrong after it fall back to llm in the validation below.
    let mismatches: Vec<String> = all_reqs
        .iter()
        .filter_map(|r| {
            let rid = r["id"].as_str().unwrap_or_default();
            let name = r["testName"].as_str().unwrap_or_default();
            let present = !name.is_empty() && tests_code.contains(name);
            let row = manifest_json["tests"]
                .as_array()
                .and_then(|a| a.iter().find(|t| t["requirement"].as_str() == Some(rid)).cloned());
            let programmatic = row.as_ref().map(|t| t["kind"].as_str() == Some("programmatic")).unwrap_or(false);
            let run = row.as_ref().and_then(|t| t["run"].as_str()).unwrap_or("").trim().to_string();
            let has_run = !run.is_empty() && runs_the_test(&run, name);
            if present && !(programmatic && has_run) {
                Some(format!(
                    "- {} [{}] is present in the tests file; declare it programmatic with a run command that invokes the tests file and selects this test (reference `{}` or the testName in the command)",
                    rid, name, tests_rel
                ))
            } else if !present && programmatic {
                Some(format!(
                    "- {} is declared programmatic but `{}` does not appear in the tests file; declare it llm or fix the name",
                    rid, name
                ))
            } else {
                None
            }
        })
        .collect();
    if !mismatches.is_empty() {
        let retry = format!(
            "{}\nYour manifest contradicts the tests file:\n{}\nReturn the corrected, complete JSON manifest object, same schema, no prose.",
            manifest_user,
            mismatches.join("\n")
        );
        if let Ok(reply2) = llm.chat(instructions, &retry, &format!("gen {} manifest retry", id)) {
            let text = strip_fences(&reply2);
            if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text[start..=end]) {
                    manifest_json = v;
                }
            }
        }
    }
    if let Some(support) = manifest_json["supportFiles"].as_array() {
        for f in support {
            let (Some(path), Some(content)) = (f["path"].as_str(), f["content"].as_str()) else { continue };
            if path.starts_with('/') || path.contains("..") {
                continue;
            }
            let p = gs.deliverable.join(path);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            snapshot_baseline(&store.out, gs, path, &mut baselined);
            std::fs::write(&p, content).map_err(|e| e.to_string())?;
            files.push(path.to_string());
        }
    }

    // Deterministic validation: a programmatic row needs its declared test present in
    // the tests artifact and a non-empty command; anything else becomes an llm row.
    let declared = manifest_json["tests"].as_array().cloned().unwrap_or_default();
    let mut tests_manifest: Vec<serde_json::Value> = Vec::new();
    for r in &all_reqs {
        let rid = r["id"].as_str().unwrap_or_default();
        let name = r["testName"].as_str().unwrap_or_default();
        let row = declared.iter().find(|t| t["requirement"].as_str() == Some(rid));
        let programmatic = row
            .map(|t| {
                let run = t["run"].as_str().unwrap_or("").trim();
                t["kind"].as_str() == Some("programmatic")
                    && !run.is_empty()
                    && tests_code.contains(name)
                    && runs_the_test(run, name)
            })
            .unwrap_or(false);
        if programmatic {
            let t = row.unwrap();
            tests_manifest.push(serde_json::json!({
                "requirement": rid, "kind": "programmatic",
                "label": t["label"].as_str().unwrap_or("test"),
                "artifact": tests_rel, "name": name,
                "run": t["run"].as_str().unwrap_or(""),
                "cwd": t["cwd"].as_str().unwrap_or("."),
                "files": [product_rel],
            }));
        } else {
            let crit_rel = r["criteriaPath"].as_str().unwrap_or_default().to_string();
            let crit_path = store.out.join("gen").join(&crit_rel);
            if let Some(p) = crit_path.parent() {
                std::fs::create_dir_all(p).ok();
            }
            let criteria = format!(
                "---\nrequirement: {}\nhash: {}\n---\n\n# Verify {}\n\nStatement: {}\n\n> {}\n\nImplementing files (under the deliverable):\n- {}\n\nConfirm the statement holds in the implementation. Verdict contract: reply PASS or FAIL with reasoning.\n",
                rid,
                r["hash"].as_str().unwrap_or_default(),
                rid,
                r["ears"].as_str().unwrap_or_default(),
                r["quote"].as_str().unwrap_or_default(),
                product_rel,
            );
            std::fs::write(&crit_path, criteria).ok();
            tests_manifest.push(serde_json::json!({
                "requirement": rid, "kind": "llm",
                "label": row.and_then(|t| t["label"].as_str()).unwrap_or("llm"),
                "artifact": crit_rel, "name": name,
                "run": format!("jazyk test {}", rid),
                "files": [product_rel],
            }));
        }
    }
    let manifest = serde_json::json!({"files": files, "tests": tests_manifest});
    crate::gen::mark(store, id, task["factHash"].as_str(), &manifest, gs)?;
    // A regeneration replaces the file set: files the previous generation recorded
    // that the new manifest no longer lists are removed, unless another entity also
    // records them. Mirrors docs/consumers/gen.md#incremental-regeneration.
    if let Some(prev) = ledger.entities.get(&own_slug) {
        for f in &prev.files {
            let kept = files.contains(f)
                || ledger.entities.iter().any(|(slug, e)| slug != &own_slug && e.files.contains(f));
            if !kept {
                snapshot_baseline(&store.out, gs, f, &mut baselined);
                std::fs::remove_file(gs.deliverable.join(f)).ok();
            }
        }
    }
    Ok(files.len())
}
