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
    // The built-in worker: "agentic" (a generation turn with file and command tools,
    // the default) or "pipeline" (the fixed file-reply sequence).
    // Mirrors docs/compiler/project-settings.md#generation.
    pub worker: String,
}

impl GenSettings {
    pub fn resolve(proj: &crate::project::Project) -> GenSettings {
        let deliverable = match &proj.gen_deliverable {
            Some(d) => proj.root.join(d),
            // Default: the project root itself. The docs glob keeps the source tree
            // out of the product's way (docs/compiler/project-settings.md#generation).
            None => proj.root.clone(),
        };
        GenSettings { deliverable, worker: proj.gen_worker.clone().unwrap_or_else(|| "agentic".into()) }
    }

    // Placeholder for sessions with no project (benchmark cases). Gen tools are absent
    // from those toolsets, so the path is never read.
    pub fn from_out(out: &Path) -> GenSettings {
        GenSettings { deliverable: out.join("gen").join("deliverable"), worker: "agentic".into() }
    }
}

// The ledger: gen/ledger.yaml. Two maps: generation state per entity, verification
// state per requirement. Mirrors docs/consumers/gen.md#the-ledger.
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Ledger {
    // Files that belong to the deliverable, not to an entity: what makes the recorded
    // commands runnable (a package.json, the entry point the build runs). Any task may
    // rewrite one; ownership never applies.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub support: Vec<String>,
    // The deliverable's decided form, written by the first run and carried by every
    // task package after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub medium: Option<Medium>,
    // Present only when the deliverable's medium must be produced by a tool.
    // Mirrors docs/consumers/gen.md#the-build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<Build>,
    #[serde(default)]
    pub entities: BTreeMap<String, EntityGen>,
    #[serde(default)]
    pub requirements: BTreeMap<String, ReqRow>,
}

// What the deliverable is made of, decided once for the whole deliverable before any
// entity generates, then carried by every task package. A per-task decision is where
// prose gets substituted for the artifact.
// Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
#[derive(Serialize, Deserialize, Clone)]
pub struct Medium {
    pub form: String,
    // written: the generated files are the deliverable.
    // built: they are the source that produces `artifact`.
    pub produced: String,
    #[serde(default)]
    pub toolchain: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub artifact: String,
}

impl Medium {
    pub fn is_built(&self) -> bool {
        self.produced == "built"
    }
    // The decision as the task packages and prompts state it: one line, no hedging.
    pub fn line(&self) -> String {
        if self.is_built() {
            format!(
                "{} produced by a build ({}). The artifact is `{}`; the files you write are the SOURCE that produces it.",
                self.form, self.toolchain, self.artifact
            )
        } else {
            format!("{} written directly ({}). The files you write ARE the deliverable.", self.form, self.toolchain)
        }
    }
}

// The one command that produces the deliverable's artifact, per deliverable, not per
// entity. Runs once before any row is judged.
#[derive(Serialize, Deserialize, Clone)]
pub struct Build {
    pub run: String,
    #[serde(default = "dot")]
    pub cwd: String,
    // Deliverable-relative paths the command creates. A missing one fails the run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub produces: Vec<String>,
    // What happened the last time the build ran. A failure is a fact the next
    // generation task must see, or it writes the same broken part again
    // (docs/consumers/gen.md#the-build). The alias reads a ledger written before the
    // key took its documented spelling.
    #[serde(default, rename = "lastRun", alias = "last_run", skip_serializing_if = "Option::is_none")]
    pub last_run: Option<BuildRun>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BuildRun {
    pub at: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
}

// Run the deliverable's build once and record what happened. Ok(()) when there is no
// build to run or it produced everything it promised; the error is what a reader
// needs to see. Mirrors docs/consumers/gen.md#the-build.
pub fn run_build(out: &Path, gs: &GenSettings, trace: &crate::turn::Trace, label: &str) -> Result<(), String> {
    let Some(b) = Ledger::load(out).build.clone() else { return Ok(()) };
    let cwd = gs.deliverable.join(&b.cwd);
    trace.line(label, &format!("build: {} (in {})", b.run, cwd.display()));
    let done = std::process::Command::new("sh")
        .arg("-c")
        .arg(&b.run)
        .current_dir(&cwd)
        .output()
        .map_err(|e| format!("build `{}` could not start in {}: {}", b.run, cwd.display(), e))?;
    if !done.status.success() {
        let mut evidence = String::from_utf8_lossy(&done.stdout).to_string();
        evidence.push_str(&String::from_utf8_lossy(&done.stderr));
        // The next generation task needs to see this; printing it here only reaches
        // whoever is watching (docs/consumers/gen.md#the-build).
        record_build_run(out, false, &evidence);
        return Err(format!(
            "build `{}` failed ({}). Output:\n{}",
            b.run,
            done.status,
            crate::llm::truncate(evidence.trim(), 2000)
        ));
    }
    let missing: Vec<&String> = b.produces.iter().filter(|p| !gs.deliverable.join(p).exists()).collect();
    if !missing.is_empty() {
        let names = missing.iter().map(|p| p.as_str()).collect::<Vec<_>>().join(", ");
        record_build_run(out, false, &format!("exited 0 but did not produce {}", names));
        return Err(format!("build `{}` exited 0 but did not produce {}", b.run, names));
    }
    record_build_run(out, true, "");
    Ok(())
}

// Record what the build did, for the next task package. Best effort: a ledger that
// cannot be written does not fail a verification run.
pub fn record_build_run(out: &Path, ok: bool, error: &str) {
    let mut ledger = Ledger::load(out);
    if let Some(b) = ledger.build.as_mut() {
        b.last_run = Some(BuildRun {
            at: crate::verify::now_iso(),
            ok,
            error: crate::llm::truncate(error.trim(), 1500),
        });
        ledger.save(out);
    }
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
    // The exit code of the last programmatic run. Present without a verdict means the
    // command ran and told us nothing: the runner failed, not the requirement
    // (docs/consumers/gen.md#a-test-that-could-not-run-says-nothing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
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

// Decide what the deliverable is made of, once, from the statements that say so. The
// answer is recorded in the ledger and stated as a fact to every later task, so no
// per-entity task has to work it out again.
// Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
pub fn decide_medium(store: &Store, llm: &crate::llm::Llm) -> Result<Medium, String> {
    // Every statement in the graph, capped: the medium is stated somewhere in the
    // documents, and which statement says it is exactly what the model must find.
    let mut statements: Vec<String> = store
        .graph
        .requirements
        .iter()
        .map(|(rid, r)| format!("- {}: {}", rid, r.ears))
        .collect();
    statements.sort();
    let mut body = String::new();
    for s in statements {
        if body.len() + s.len() > 12_000 {
            break;
        }
        body.push_str(&s);
        body.push('\n');
    }
    let entities: Vec<&str> = store.graph.entities.values().map(|e| e.name.as_str()).take(40).collect();
    let system = "You decide what a deliverable is made of. Answer with one JSON object and nothing else.";
    let user = format!(
        "These statements are the whole specification of one deliverable.\n\n{}\n\nEntities: {}\n\n\
         Decide the deliverable's medium from the statements alone.\n\
         - `form`: what the deliverable is, in a few words (e.g. `Rust library`, `Microsoft PowerPoint deck`, `printed book`).\n\
         - `produced`: `written` when the files a generator writes ARE the deliverable (source code, a manuscript, a configuration). \
         `built` when the medium is a format a tool must produce (a slide deck, a PDF, an image, a compiled binary): the files are the source, and a command turns them into the artifact.\n\
         - `toolchain`: what writes or builds it (e.g. `rustc and cargo test`, `python3 with python-pptx`). Name a library that can actually emit the format.\n\
         - `artifact`: for `built` only, the file the build produces, relative to the deliverable directory (e.g. `jazyk.pptx`). Empty for `written`.\n\n\
         Reply with exactly: {{\"form\": \"...\", \"produced\": \"written\"|\"built\", \"toolchain\": \"...\", \"artifact\": \"...\"}}",
        body,
        entities.join(", ")
    );
    let mut last = String::new();
    for attempt in 0..2 {
        let ask = if attempt == 0 {
            user.clone()
        } else {
            format!("{}\n\nYour previous answer was rejected: {}. Reply with the JSON object only.", user, last)
        };
        let reply = llm.chat(system, &ask, "gen medium", if attempt == 0 { "decide" } else { "decide retry" })?;
        let raw = crate::llm::extract_json_object(&reply).unwrap_or(reply);
        let v: Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                last = format!("not JSON ({})", e);
                continue;
            }
        };
        let produced = v["produced"].as_str().unwrap_or_default().trim().to_lowercase();
        if produced != "written" && produced != "built" {
            last = format!("`produced` was `{}`, not `written` or `built`", produced);
            continue;
        }
        let artifact = v["artifact"].as_str().unwrap_or_default().trim().trim_start_matches("./").to_string();
        if produced == "built" && artifact.is_empty() {
            last = "`built` needs an `artifact` path".into();
            continue;
        }
        let form = v["form"].as_str().unwrap_or_default().trim().to_string();
        if form.is_empty() {
            last = "`form` was empty".into();
            continue;
        }
        return Ok(Medium {
            form,
            produced,
            toolchain: v["toolchain"].as_str().unwrap_or_default().trim().to_string(),
            artifact,
        });
    }
    Err(format!("could not decide the deliverable's medium: {}", last))
}

// The generation contract, identical for every worker. It never names a medium: what
// the deliverable is (a language, a format, a genre) is a fact the documents state,
// carried by the context pack.
pub fn instructions() -> String {
    format!(
        "Generate the entity's part of the deliverable AND the tests for its requirements.\n\
         - Derive the medium from the requirements and the context: what the documents say the deliverable is decides what you write. You choose the layout, the file names, and the build or support files that make your recorded commands executable; every file you write must appear in the manifest you pass to gen_mark.\n\
         - The deliverable is the artifact itself, never a description of it. A requirement naming a format, a medium, or a piece of content is an obligation to PRODUCE that thing. Writing a document that says the artifact will be in some format, or a manifest listing what the artifact would contain, satisfies nothing. When the medium is text the requirements describe directly (source code, a manuscript, a configuration), your files are the deliverable. When the medium must be produced by a tool (a slide deck, a PDF, a rendered site, a compiled binary, an image), write the source that produces it and return a `build` in the manifest: {{run, cwd, produces}} where `run` executes from the deliverable directory and `produces` lists the artifact paths it creates. The harness runs the build before any test. One build per deliverable: if the package already carries a `build`, reuse it and extend its source instead of recording a second one.\n\
         - Content requirements are satisfied by content. A requirement saying the artifact shows a title, states a definition, or uses a color is met only when the artifact carries that exact title, that definition, that color value. Never write placeholder filler (`[Project Goal]`, `Lorem ipsum`, `TODO`) in place of what a requirement states. If a requirement does not say what the content is, implement exactly what it does say and nothing more.\n\
         - One toolchain per deliverable: the run commands already recorded (the package lists them) establish the language, the test runner, and the command style; reuse them exactly. Never introduce a second test runner.\n\
         - Every deliverable file belongs to one entity. Never write to a file another entity's task produced (the package lists them); reference it through imports instead and pick a path of your own.\n\
         - Each recorded run command must execute from the deliverable directory as recorded. If it needs a build or configuration file that no task has written yet (package.json, Cargo.toml), return that file as a support file in the manifest step.\n\
         - Every requirement is an obligation; implement each and place a single-line marker comment directly above the implementing site: `req:<id> hash:<hash8>` in the medium's comment syntax, nothing else on that line. The harness strips marker lines from your files and records each location in the ledger, so the delivered file stays clean.\n\
         - Derive one test per requirement. Pick the kind per requirement:\n\
           - programmatic: any test a command can run (unit, integration, cucumber are examples, not a taxonomy). Write the test into the deliverable and record the exact command that runs only that test, exactly as it must be executed from the deliverable directory. Its exit code is the verdict, so the artifact must propagate failure: a harness that prints a failure and still exits zero verifies nothing. The test must inspect the artifact the requirement is about, never a document that describes it: asserting that a manifest names a format or that a plan lists a feature is circular, since both sides are your own prose. When a build produces the artifact, open what the build produced. A test must run with no fixture the deliverable does not define; if it needs setup, write that setup file and list it in the manifest. A test must be falsifiable: its assertion has to fail when the requirement is violated. If the only assertion you can write would pass either way, the requirement is not programmatically testable from this artifact; declare it llm instead of writing a test that always passes.\n\
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
// The file the build command runs, with its current content. The command names it as
// an argument, so the entry is the first token that exists under the deliverable.
// Mirrors docs/consumers/gen.md#the-build.
fn build_entry(ledger: &Ledger, gs: &GenSettings) -> Value {
    let Some(b) = &ledger.build else { return Value::Null };
    let dir = gs.deliverable.join(&b.cwd);
    for token in b.run.split_whitespace() {
        let token = token.trim_matches(|c| c == '"' || c == '\'');
        if token.starts_with('-') || token.is_empty() {
            continue;
        }
        let candidate = dir.join(token);
        if candidate.is_file() {
            let rel = norm_rel(&pathdiff(&candidate, &gs.deliverable).unwrap_or_else(|| token.to_string()));
            let content = std::fs::read_to_string(&candidate).unwrap_or_default();
            return json!({"path": rel, "content": crate::llm::truncate(&content, 12_000)});
        }
    }
    Value::Null
}

// The file a build command runs, by name alone: the first token that looks like a
// path rather than a flag or an interpreter.
fn entry_from_run(run: &str) -> Option<String> {
    run.split_whitespace()
        .map(|t| t.trim_matches(|c| c == '"' || c == '\''))
        .find(|t| !t.starts_with('-') && t.contains('.') && !t.ends_with("python") && !t.ends_with("python3"))
        .map(String::from)
}

// Deliverable-relative paths as the manifest writes them: no leading `./`, no
// duplicate separators. Joining a build's `cwd` produces the other spelling, and a
// path compared in the wrong spelling silently matches nothing.
fn norm_rel(p: &str) -> String {
    p.replace("/./", "/").trim_start_matches("./").to_string()
}

// A deliverable-relative path, lexically.
fn pathdiff(path: &Path, base: &Path) -> Option<String> {
    path.strip_prefix(base).ok().map(|p| p.to_string_lossy().replace('\\', "/"))
}

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
    // The manifest of other tasks' work. A composite deliverable is assembled from parts
    // other entities wrote, so a path alone is not enough: the entry names what the files
    // hold, by the statements the task implemented.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    let manifest: BTreeMap<&String, Value> = ledger
        .entities
        .iter()
        .map(|(k, v)| {
            let holds: Vec<String> = v
                .requirements
                .iter()
                .filter_map(|rid| store.graph.requirements.get(rid).map(|r| r.ears.clone()))
                .collect();
            // Under a built medium the entry this task rewrites has to call into
            // these files, so their content travels with them: a path and a
            // statement do not say what a part is called
            // (docs/consumers/gen.md#the-build).
            let show = ledger.medium.as_ref().map(|m| m.is_built()).unwrap_or(false);
            let contents: BTreeMap<String, String> = if show {
                v.files
                    .iter()
                    .filter(|f| !f.contains("test"))
                    .filter_map(|f| {
                        std::fs::read_to_string(gs.deliverable.join(f))
                            .ok()
                            .map(|c| (f.clone(), crate::llm::truncate(&c, 4_000)))
                    })
                    .collect()
            } else {
                BTreeMap::new()
            };
            (k, json!({"files": v.files, "holds": holds, "contents": contents}))
        })
        .collect();
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
        "supportFiles": ledger.support,
        // The entry the build runs, with what it holds today: a later task returns it
        // updated so the artifact carries its part too
        // (docs/consumers/gen.md#the-build).
        "buildEntry": build_entry(&ledger, gs),
        // A standing build failure: the artifact does not exist until it is fixed. The
        // package says whether this entity's own files are what the failure names, so
        // the task that can fix it knows it is the one (docs/consumers/gen.md#the-build).
        "buildError": ledger
            .build
            .as_ref()
            .and_then(|b| b.last_run.as_ref())
            .filter(|r| !r.ok)
            .map(|r| {
                let mine: Vec<String> = ledger
                    .entities
                    .get(&slug)
                    .map(|e| e.files.iter().filter(|f| r.error.contains(f.as_str())).cloned().collect())
                    .unwrap_or_default();
                json!({"at": r.at, "error": r.error, "yours": mine})
            }),
        // Decided once for the deliverable; a task states it, never re-decides it.
        "medium": ledger.medium.as_ref().map(|m| json!({
            "form": m.form, "produced": m.produced, "toolchain": m.toolchain, "artifact": m.artifact,
        })),
        "build": ledger.build.as_ref().map(|b| json!({"run": b.run, "cwd": b.cwd, "produces": b.produces})),
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
    // Support files are the deliverable's, recorded once and rewritable by any later
    // task (docs/consumers/gen.md#file-ownership-and-conventions).
    let support: Vec<String> = manifest["supportFiles"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from).or_else(|| x["path"].as_str().map(String::from)))
                .collect()
        })
        .unwrap_or_default();

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
    // One build per deliverable: the first task that needs one establishes it, later
    // tasks receive it in their package and reuse it.
    // Mirrors docs/consumers/gen.md#the-build.
    if ledger.build.is_none() {
        match manifest["build"]["run"].as_str().map(str::trim).filter(|r| !r.is_empty()) {
            Some(run) => {
                let mut produces: Vec<String> = manifest["build"]["produces"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                // A built medium names its artifact; a build that forgets to list it
                // would pass its own check while producing nothing.
                if let Some(m) = ledger.medium.as_ref().filter(|m| m.is_built()) {
                    if !produces.iter().any(|p| p.trim_start_matches("./") == m.artifact) {
                        produces.push(m.artifact.clone());
                    }
                }
                ledger.build = Some(Build {
                    run: run.to_string(),
                    cwd: manifest["build"]["cwd"].as_str().unwrap_or(".").to_string(),
                    produces,
                    last_run: None,
                });
            }
            // Under a built medium the build is the only thing that makes an artifact
            // exist, so a manifest without one is rejected, not quietly accepted
            // (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
            None => {
                if let Some(m) = ledger.medium.as_ref().filter(|m| m.is_built()) {
                    return Err(format!(
                        "this deliverable is {} and no build is recorded: return `build` with the command that produces `{}`, its `cwd`, and `produces` listing `{}`",
                        m.form, m.artifact, m.artifact
                    ));
                }
            }
        }
    }
    for path in &support {
        if !ledger.support.iter().any(|p| p == path) {
            ledger.support.push(path.clone());
        }
    }
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
                    exit_code: None,
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
        let gs = GenSettings { deliverable: out.join("product"), worker: "agentic".into() };
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

    // One build per deliverable: the first task that needs one establishes it, later
    // tasks receive it in their package and cannot replace it.
    // Mirrors docs/consumers/gen.md#the-build.
    #[test]
    fn build_is_recorded_once_and_carried_into_later_packages() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-build-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        assert!(task_package(&s, "ent:cart", &gs).unwrap()["build"].is_null());

        std::fs::create_dir_all(gs.deliverable.join("src")).ok();
        std::fs::write(gs.deliverable.join("src/cart.rs"), "// product").ok();
        mark(
            &s,
            "ent:cart",
            None,
            &serde_json::json!({
                "files": ["src/cart.rs"],
                "build": {"run": "python build_deck.py", "cwd": ".", "produces": ["deck.pptx"]},
                "tests": [],
            }),
            &gs,
        )
        .unwrap();
        let b = Ledger::load(&out).build.unwrap();
        assert_eq!(b.run, "python build_deck.py");
        assert_eq!(b.produces, vec!["deck.pptx".to_string()]);
        // The next task sees it and a second recording does not take.
        assert_eq!(task_package(&s, "ent:cart", &gs).unwrap()["build"]["run"], "python build_deck.py");
        mark(
            &s,
            "ent:cart",
            None,
            &serde_json::json!({"files": ["src/cart.rs"], "build": {"run": "make all"}, "tests": []}),
            &gs,
        )
        .unwrap();
        assert_eq!(Ledger::load(&out).build.unwrap().run, "python build_deck.py");
        std::fs::remove_dir_all(&out).ok();
    }

    // A built medium makes the build an obligation: a manifest without one is
    // rejected, and a recorded one always lists the decided artifact.
    // Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
    // A reply that writes its module and the entry in one answer has written both.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    #[test]
    fn a_reply_can_carry_more_than_one_file() {
        let one = parse_file_replies("FILE: src/a.py\nprint(1)\n").unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].0, "src/a.py");
        assert_eq!(one[0].1.trim(), "print(1)");

        // A fence around the body is the model's markdown habit, not content.
        let fenced = parse_file_replies("FILE: src/a.py\n```python\nprint(1)\n```\n").unwrap();
        assert_eq!(fenced[0].1.trim(), "print(1)");

        let two = parse_file_replies("FILE: src/a.py\nprint(1)\n\nFILE: build.py\nimport a\na.go()\n").unwrap();
        assert_eq!(two.len(), 2);
        assert_eq!(two[0].0, "src/a.py");
        assert_eq!(two[0].1.trim(), "print(1)");
        assert_eq!(two[1].0, "build.py");
        assert_eq!(two[1].1.trim(), "import a\na.go()");

        // An escaping path is dropped, not written.
        let bad = parse_file_replies("FILE: src/a.py\nx\nFILE: ../outside.py\ny\n").unwrap();
        assert_eq!(bad.len(), 1);
        assert!(parse_file_replies("no file line here").is_err());
    }

    // The extra files a reply wrote are sorted by what the manifest says, not by the
    // order they arrived in: the build's entry and declared support are the
    // deliverable's, a second module or test file is the entity's.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    #[test]
    fn extra_files_are_classified_by_the_manifest() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-extra-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        std::fs::create_dir_all(gs.deliverable.join("src")).ok();
        for f in ["src/cart.rs", "src/cart_util.rs", "build.rs", "Cargo.toml"] {
            std::fs::write(gs.deliverable.join(f), "// x").ok();
        }
        // The task wrote its part plus three more; the manifest calls two of them the
        // deliverable's (one as support, one as the build's entry).
        mark(
            &s,
            "ent:cart",
            None,
            &serde_json::json!({
                "files": ["src/cart.rs", "src/cart_util.rs"],
                "supportFiles": ["Cargo.toml"],
                "build": {"run": "cargo build --release", "cwd": "."},
                "tests": [],
            }),
            &gs,
        )
        .unwrap();
        let ledger = Ledger::load(&out);
        assert_eq!(ledger.support, vec!["Cargo.toml".to_string()]);
        let mine = &ledger.entities["cart"].files;
        assert!(mine.contains(&"src/cart_util.rs".to_string()), "{:?}", mine);
        assert!(!mine.contains(&"Cargo.toml".to_string()), "{:?}", mine);
        std::fs::remove_dir_all(&out).ok();
    }

    #[test]
    fn built_medium_demands_a_build_that_names_the_artifact() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-medium-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        let mut ledger = Ledger::load(&out);
        ledger.medium = Some(Medium {
            form: "Microsoft PowerPoint deck".into(),
            produced: "built".into(),
            toolchain: "python3 with python-pptx".into(),
            artifact: "jazyk.pptx".into(),
        });
        ledger.save(&out);
        // The decision reaches the task package.
        let task = task_package(&s, "ent:cart", &gs).unwrap();
        assert_eq!(task["medium"]["produced"], "built");
        assert_eq!(task["medium"]["artifact"], "jazyk.pptx");

        std::fs::create_dir_all(gs.deliverable.join("src")).ok();
        std::fs::write(gs.deliverable.join("src/cart.rs"), "// product").ok();
        let no_build = serde_json::json!({"files": ["src/cart.rs"], "tests": []});
        let err = mark(&s, "ent:cart", None, &no_build, &gs).unwrap_err();
        assert!(err.contains("no build is recorded"), "{}", err);

        // A build that forgets the artifact still produces it: the decision fills it in.
        mark(
            &s,
            "ent:cart",
            None,
            &serde_json::json!({
                "files": ["src/cart.rs"],
                "build": {"run": "python3 build_deck.py", "cwd": "."},
                "tests": [],
            }),
            &gs,
        )
        .unwrap();
        let b = Ledger::load(&out).build.unwrap();
        assert_eq!(b.produces, vec!["jazyk.pptx".to_string()]);
        std::fs::remove_dir_all(&out).ok();
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
    limits: &crate::project::Limits,
    lint: &crate::project::Linting,
    trace: &crate::turn::Trace,
) -> Result<Value, String> {
    use crate::turn::TraceEvent;
    // Every prompt this run sends reports under the entity it is generating.
    let llm = &llm.with_trace(trace);
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

    // The medium is decided before the first task, not inside it. A ledger with no
    // entities decides again, so wiping the deliverable is how a project changes its
    // mind (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
    {
        let mut ledger = Ledger::load(&store.out);
        if ledger.medium.is_none() || ledger.entities.is_empty() {
            let medium = decide_medium(store, llm)?;
            trace.line("gen medium", &medium.line());
            ledger.medium = Some(medium);
            ledger.save(&store.out);
        }
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
        let result = if gs.worker == "pipeline" {
            gen_one(store, llm, gs, id, &task)
        } else {
            gen_turn(store, llm, gs, id, limits, lint, trace)
        };
        match result {
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
    // A built deliverable is not generated until the artifact exists, so the run ends
    // by producing it. The failure is recorded either way, which is what the next run
    // reads (docs/consumers/gen.md#the-build).
    let build = match run_build(&store.out, gs, trace, "gen") {
        Ok(()) => {
            let produced = Ledger::load(&store.out)
                .build
                .map(|b| b.produces.join(", "))
                .filter(|p| !p.is_empty());
            if let Some(p) = &produced {
                trace.line("gen", &format!("built {}", p));
            }
            json!({ "ok": true, "produced": produced })
        }
        Err(e) => {
            trace.line("gen", &e);
            json!({ "ok": false, "error": e })
        }
    };
    Ok(json!({ "regenerated": regenerated, "skipped": skipped, "failures": failures, "build": build }))
}

// Every file a reply carries, in order. A step asks for one, but a model that writes
// its module and the entry point in a single answer has written both, and folding the
// second into the first leaves a file that cannot parse.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
// The agentic worker: one entity as a generation turn on the harness, with the file
// and command tools. Success is the ledger's word, never the model's: the turn must
// have left record_generation's mark with current facts and existing files.
// Mirrors docs/compiler/turns.md#generation-turns.
fn gen_turn(
    store: &Store,
    llm: &crate::llm::Llm,
    gs: &GenSettings,
    id: &str,
    limits: &crate::project::Limits,
    lint: &crate::project::Linting,
    trace: &crate::turn::Trace,
) -> Result<usize, String> {
    let item = crate::model::WorkItem {
        task: "generate-entity".into(),
        target: id.to_string(),
        dirty_sections: Vec::new(),
        stale_anchors: Vec::new(),
    };
    let out = crate::turn::run_turn(llm, store.clone(), &item, limits, lint, gs, trace);
    if let Some(e) = out.failed {
        return Err(e);
    }
    let ledger = Ledger::load(&store.out);
    let hash = fact_hash(store, id);
    match ledger.entities.get(&slug_of(id)) {
        Some(e) if e.fact_hash == hash && !e.files.is_empty() && e.files.iter().all(|f| gs.deliverable.join(f).exists()) => {
            Ok(e.files.len())
        }
        Some(e) if e.fact_hash != hash => Err(format!(
            "the turn recorded factHash {} but the graph says {}; the task package carries the current one",
            e.fact_hash, hash
        )),
        Some(_) => Err("the turn recorded files that do not exist under the deliverable".into()),
        None => Err("the turn ended without record_generation; nothing landed in the ledger".into()),
    }
}

pub fn parse_file_replies(reply: &str) -> Result<Vec<(String, String)>, String> {
    let (first_path, rest) = parse_file_reply(reply)?;
    let mut out = Vec::new();
    let mut path = first_path;
    let mut body: Vec<&str> = Vec::new();
    for line in rest.lines() {
        match line.trim_start().strip_prefix("FILE:") {
            Some(next) if !next.trim().is_empty() => {
                out.push((path, body.join("\n")));
                path = next.trim().to_string();
                body = Vec::new();
            }
            _ => body.push(line),
        }
    }
    out.push((path, body.join("\n")));
    // The reply's outer layer is the FILE line, so a fence the model wrapped the file
    // in survives that first strip. Each file's own body gets unwrapped here, or the
    // artifact starts with ```python and parses as nothing.
    for (_, body) in out.iter_mut() {
        *body = strip_fences(body);
    }
    // A path that escapes the deliverable is not a file this harness writes.
    out.retain(|(p, _)| !p.is_empty() && !p.starts_with('/') && !p.contains(".."));
    if out.is_empty() {
        return Err("no usable file path in the reply".into());
    }
    Ok(out)
}

pub fn parse_file_reply(reply: &str) -> Result<(String, String), String> {
    let reply = strip_fences(reply);
    // The contract is a FILE line first. A model that opens with a sentence or a
    // fence has still answered, so the line is taken where it stands; anything before
    // it is preamble, and the file starts after it.
    let reply = match reply.lines().position(|l| l.trim_start().starts_with("FILE:")) {
        Some(0) | None => reply.clone(),
        Some(n) => reply.lines().skip(n).collect::<Vec<_>>().join("\n"),
    };
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

// What the decided medium demands of the file this task writes. The decision is
// already made; this states what follows from it, in the imperative.
fn medium_directive(medium: &Option<Medium>) -> String {
    match medium {
        Some(m) if m.is_built() => format!(
            "The deliverable is {}. You cannot type that format out, so the file you write is the SOURCE THAT PRODUCES IT: real, runnable {} that contributes this entity's part of `{}`. One entry point, recorded as the build, runs every entity's part and writes the artifact, so write your part as something that entry can include: a function it calls, a module it imports. Do NOT write a document describing the artifact, an outline of it, a placeholder, or a manifest listing what it would contain: prose about a deck is not a deck, and it satisfies nothing. Do NOT write the artifact file itself by hand.",
            m.form, if m.toolchain.is_empty() { "source" } else { &m.toolchain }, m.artifact
        ),
        Some(m) => format!(
            "The deliverable is {}, written directly: the file you write IS the artifact{}.",
            m.form,
            if m.toolchain.is_empty() { String::new() } else { format!(" ({})", m.toolchain) }
        ),
        // No decision on record (an external worker's package): fall back to deriving it.
        None => "Decide the medium from the requirements above: when they name a format you cannot type out (a slide deck, a PDF, an image, a compiled binary), write the SOURCE that produces it and record its build in the manifest step; otherwise the file you write is the artifact.".to_string(),
    }
}

pub fn gen_one(store: &Store, llm: &crate::llm::Llm, gs: &GenSettings, id: &str, task: &serde_json::Value) -> Result<usize, String> {
    let mut baselined: std::collections::HashSet<String> = Default::default();
    let instructions = task["instructions"].as_str().unwrap_or_default();
    // The deliverable's medium is decided before any task runs; this task states it
    // and never re-decides it
    // (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
    let medium: Option<Medium> = serde_json::from_value(task["medium"].clone()).ok();
    let medium_line = match &medium {
        Some(m) => m.line(),
        None => "not decided; derive it from the requirements".to_string(),
    };
    let run_commands = task["runCommands"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    let header = format!(
        "The deliverable: {}\n{}\nEntity {} ({})\nContext:\n{}\nChanged since last generation: {}\nWhat other entities' tasks already wrote, by entity: their `files` and the statements those files `holds`. Each file belongs to its entity; never write to one of them. Reference them instead: when your part composes theirs, read or import those paths, and let what they hold tell you what is in them: {}\nRecorded run commands (the established toolchain; reuse it): {}\nRecorded build for this deliverable: {}\n",
        medium_line,
        // A standing build failure means the artifact does not exist: whoever
        // regenerates next has to fix what broke (docs/consumers/gen.md#the-build).
        match &task["buildError"] {
            serde_json::Value::Null => String::new(),
            e => {
                let yours: Vec<&str> = e["yours"].as_array().map(|a| a.iter().filter_map(|f| f.as_str()).collect()).unwrap_or_default();
                format!(
                    "\nTHE BUILD IS CURRENTLY BROKEN, so the artifact does not exist. It last failed with:\n{}\n{}Write source that runs against the real library API: import the library the toolchain names, exactly as it is spelled, and call the functions it really has. Do not repeat a call or an import the failure above already rejected.\n",
                    e["error"].as_str().unwrap_or_default(),
                    if yours.is_empty() {
                        "The failure is not in a file you own here, but do not add another like it. ".to_string()
                    } else {
                        format!("THE FAILURE IS IN YOUR OWN FILE ({}). Fixing it is this task's first job. ", yours.join(", "))
                    }
                )
            }
        },
        id,
        task["name"].as_str().unwrap_or_default(),
        task["context"].as_str().unwrap_or_default(),
        task["changed"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
        serde_json::to_string(&task["generatedFiles"]).unwrap_or_default(),
        if run_commands.is_empty() { "(none yet; this task establishes them)" } else { &run_commands },
        match &task["build"] {
            serde_json::Value::Null => "(none yet)".to_string(),
            b => b.to_string(),
        },
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

    // Files a reply wrote beside the step's own file. Whether each belongs to this
    // entity or to the deliverable is the manifest's call, made below
    // (docs/consumers/gen.md#file-ownership-and-conventions).
    let mut extra_written: Vec<String> = Vec::new();
    // Product content; the model names the file.
    let mut code = String::new();
    let mut product_rel = String::new();
    for (k, group) in groups.iter().enumerate() {
        let req_lines: Vec<String> = group.as_array().map(|a| a.iter().map(req_line).collect()).unwrap_or_default();
        let user = if k == 0 {
            format!(
                "{}\n{}\nWrite the implementing content for this entity. Choose the file path yourself, relative to the deliverable. Reply with the first line exactly `FILE: <path>` and the file content after it. Write more files the same way when this part needs them (a dependency manifest, an entry point, a second module): another `FILE: <path>` line on its own, then that file. The first file is this entity's part. Put a single-line marker comment (req:<id> hash:<hash8> in the medium's comment syntax, alone on its line) directly above each implementing site; the harness strips it and records the location. Requirements (group 1 of {}):\n{}\n",
                header, medium_directive(&medium), parts, req_lines.join("\n")
            )
        } else {
            format!(
                "{}\nRequirements (group {} of {}):\n{}\n\nThe file `{}` so far:\n{}\nReturn ONLY additional content to append, no FILE line.",
                header, k + 1, parts, req_lines.join("\n"), product_rel, crate::llm::truncate(&code, 20_000)
            )
        };
        let reply = llm
            .chat(instructions, &user, &format!("gen {}", id), &format!("product {}/{}", k + 1, parts))
            .map_err(|e| format!("product part {}/{}: {}", k + 1, parts, e))?;
        if k == 0 {
            // Shape gets one corrective round here too: the product step is where a
            // long prompt most often costs the FILE line
            // (docs/consumers/gen.md#file-ownership-and-conventions).
            let mut written = match parse_file_replies(&reply) {
                Ok(v) => v,
                Err(e) => {
                    let retry = format!(
                        "{}\nYour reply was not in the required shape ({}). This step takes no JSON. Reply exactly like this, the FILE line first and the file after it:\n\nFILE: path/of/your/choice.ext\n<the content>\n",
                        user, e
                    );
                    let again = llm
                        .chat(instructions, &retry, &format!("gen {}", id), "product format retry")
                        .map_err(|e| format!("product format retry: {}", e))?;
                    parse_file_replies(&again)?
                }
            };
            // The first file is this entity's part. Anything else the reply wrote is
            // deliverable-wide (an entry point, a config), so it lands as a support
            // file instead of being folded into the product and breaking it.
            let extra: Vec<(String, String)> = written.split_off(1);
            let (mut path, mut body) = written.remove(0);
            for (p, content) in extra {
                if owner_of(&p).is_some() {
                    continue;
                }
                let full = gs.deliverable.join(&p);
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                snapshot_baseline(&store.out, gs, &p, &mut baselined);
                if std::fs::write(&full, format!("{}\n", content.trim_end())).is_ok() {
                    extra_written.push(p);
                }
            }
            if let Some(owner) = owner_of(&path) {
                let retry = format!(
                    "{}\nThe path `{}` already belongs to entity `{}`; never write to another entity's file. Reply again, same content, under a file path of your own: first line exactly `FILE: <path>`, content after it.",
                    user, path, owner
                );
                let reply2 = llm
                    .chat(instructions, &retry, &format!("gen {}", id), "product retry")
                    .map_err(|e| format!("product retry: {}", e))?;
                let (p, b) = parse_file_reply(&reply2).map(|(p, b)| (p, b))?;
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
        "{}\nWrite the tests for the requirements against `{}` (content below). Choose the test file path yourself. One test per requirement you can test programmatically, named EXACTLY by its [testName], with the single-line marker comment above it. Reply with the first line exactly `FILE: <path>` and the content after it; more files follow the same way, each after its own `FILE: <path>` line, when the tests need them (a fixture, a conftest). The first file is the tests artifact. If no requirement can be tested programmatically, reply with exactly `NONE`.\nA test must be falsifiable: its assertion has to FAIL when the requirement is violated. Before writing one, ask what change to the artifact would break this requirement, and assert on exactly that. Asserting on unrelated text, on a heading, or on a keyword that is present either way is worse than no test: it reports a pass while the requirement is unmet. If a requirement is about something this artifact does not carry, do NOT invent a stand-in assertion for it. Leave it out of this file; the manifest step declares it `llm` and a human or an agent judges it.\n{}\nRequirements:\n{}\n\nThe product file:\n{}\n",
        header,
        match &medium {
            Some(m) if m.is_built() => format!(
                "The artifact is `{}`, produced by the build. Every test opens that file and asserts on what is inside it. A test that reads the source that produces it, or any document beside it, verifies nothing.",
                m.artifact
            ),
            _ => "When the artifact is produced by a build, the test opens what the build produced, never the source that produces it.".to_string(),
        },
        product_rel,
        req_lines.join("\n"),
        crate::llm::truncate(&code, 16_000)
    );
    let tests_reply = llm
        .chat(instructions, &tests_user, &format!("gen {}", id), "tests")
        .map_err(|e| format!("tests file: {}", e))?;
    let mut files = vec![product_rel.clone()];
    // Files the manifest declares as the deliverable's, filled in once it is parsed
    // (docs/consumers/gen.md#file-ownership-and-conventions).
    let mut support_files: Vec<String> = Vec::new();
    let mut tests_rel = String::new();
    let mut tests_code = String::new();
    if tests_reply.trim() != "NONE" {
        // The shape is the harness's contract, and a weak model drops it under a long
        // prompt before it gets the content wrong: one corrective round, then fail.
        let parsed = match parse_file_replies(&tests_reply) {
            Ok(mut v) => {
                // Same rule as the product step: the first file is the tests
                // artifact, anything else the reply wrote is deliverable-wide.
                let extra: Vec<(String, String)> = v.split_off(1);
                for (p, content) in extra {
                    if owner_of(&p).is_some() {
                        continue;
                    }
                    let full = gs.deliverable.join(&p);
                    if let Some(parent) = full.parent() {
                        std::fs::create_dir_all(parent).ok();
                    }
                    snapshot_baseline(&store.out, gs, &p, &mut baselined);
                    if std::fs::write(&full, format!("{}\n", content.trim_end())).is_ok() {
                        extra_written.push(p);
                    }
                }
                Some(v.remove(0))
            }
            Err(e) => {
                let retry = format!(
                    "{}\nYour reply was not in the required shape ({}). This step is not the manifest step and takes no JSON. Reply exactly like this, the FILE line first and the test file after it:\n\nFILE: tests/test_example.py\nimport unittest\n\nclass TestExample(unittest.TestCase):\n    def test_name_from_the_list(self):\n        ...\n\nOr reply with exactly NONE when no requirement here can be tested programmatically.",
                    tests_user, e
                );
                let again = llm
                    .chat(instructions, &retry, &format!("gen {}", id), "tests format retry")
                    .map_err(|e| format!("tests format retry: {}", e))?;
                if again.trim() == "NONE" {
                    None
                } else {
                    Some(parse_file_reply(&again).map_err(|e| format!("tests reply: {}", e))?)
                }
            }
        };
        if let Some((mut path, mut body)) = parsed {
        if let Some(owner) = owner_of(&path) {
            let retry = format!(
                "{}\nThe path `{}` already belongs to entity `{}`; never write to another entity's file. Reply again, same content, under a file path of your own: first line exactly `FILE: <path>`, content after it.",
                tests_user, path, owner
            );
            let reply2 = llm
                .chat(instructions, &retry, &format!("gen {}", id), "tests retry")
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
        "Files written so far for entity {}: {:?} under the deliverable directory `{}`.\nRecorded run commands already in use (reuse this toolchain, never introduce a second test runner): {}\nBuild already recorded for this deliverable: {}\nThe build's entry point and its current content (rewrite it in supportFiles so it includes your part; empty when there is no build yet): {}\nTest names the harness found in the tests file (declare each of these programmatic with its run command; declare a requirement whose name is absent as llm): {}\nEach programmatic run command must invoke the tests artifact and select only that test: the command text must reference the tests file path or the testName. A command that only runs the product is invalid.\nReturn ONLY a JSON object, no prose:\n{{\"supportFiles\": [{{\"path\": \"...\", \"content\": \"...\"}}], \"build\": {{\"run\": \"...\", \"cwd\": \".\", \"produces\": [\"...\"]}}, \"tests\": [{{\"requirement\": \"req:...\", \"kind\": \"programmatic\"|\"llm\", \"label\": \"your words\", \"name\": \"the testName\", \"run\": \"exact command executed from the deliverable directory that runs only that test\", \"cwd\": \".\"}}]}}\nsupportFiles are build or configuration files required for the run commands to execute (empty array if none are needed or they already exist). A run command that cannot execute from a fresh checkout of the deliverable is a defect: if it needs a runner or build file no listed file provides (a package.json for npx jest, a Cargo.toml for cargo test), you MUST return that file in supportFiles.\n{} Every requirement must appear once in tests. Requirements and test names:\n{}\n\nThe tests file `{}`:\n{}\n",
        id,
        files,
        task["deliverable"].as_str().unwrap_or_default(),
        if run_commands.is_empty() { "(none yet; this task establishes the toolchain)" } else { &run_commands },
        match &task["build"] {
            serde_json::Value::Null => "(none; record one only if the medium must be built)".to_string(),
            b => b.to_string(),
        },
        match &task["buildEntry"] {
            serde_json::Value::Null => "(none yet)".to_string(),
            e => format!(
                "`{}`:\n{}",
                e["path"].as_str().unwrap_or_default(),
                e["content"].as_str().unwrap_or_default()
            ),
        },
        if found_names.is_empty() { "(none)".to_string() } else { found_names.join(", ") },
        // The medium is already decided, so the build is an obligation or a
        // non-question; it is never the worker's call
        // (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
        match (&medium, &task["build"]) {
            (Some(m), serde_json::Value::Null) if m.is_built() => format!(
                "`build` is REQUIRED here and no build is recorded yet: this deliverable is {}, so return `build` with the exact command that produces `{}`, `cwd` relative to the deliverable, and `produces` listing `{}`. What the build runs must be a NEW support file you return in supportFiles, never the product file above: every later entity adds its own part to the artifact by rewriting that entry, and it cannot rewrite a file you own. So return the entry as a support file that IMPORTS the product file you wrote and calls into it, then writes `{}`. The entry composes parts; it never re-implements one. A manifest without the build is rejected.",
                m.form, m.artifact, m.artifact, m.artifact
            ),
            (_, serde_json::Value::Null) => "Omit `build` entirely: the files you wrote are themselves the deliverable.".to_string(),
            (Some(m), _) if m.is_built() => format!(
                "A build is already recorded; omit `build` and reuse it. Its entry point is shown above with its current content: your part is NOT in the artifact until that entry imports your file and calls into it, so return the entry in supportFiles, rewritten to import your product file the same way it imports the others, still producing `{}`. Keep every part it already composes, and never re-implement a part inside the entry.",
                m.artifact
            ),
            _ => "A build is already recorded for this deliverable; omit `build` and reuse it.".to_string(),
        },
        all_reqs.iter().map(|r| format!("- {} [{}]", r["id"].as_str().unwrap_or(""), r["testName"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("\n"),
        tests_rel,
        crate::llm::truncate(&tests_code, 12_000)
    );
    let manifest_reply = llm
        .chat(instructions, &manifest_user, &format!("gen {}", id), "manifest")
        .map_err(|e| format!("manifest: {}", e))?;
    let parse_manifest = |reply: &str| -> Result<serde_json::Value, String> {
        let text = strip_fences(reply);
        let start = text.find('{').ok_or("the reply held no JSON object")?;
        let end = text.rfind('}').ok_or("the reply held no JSON object")?;
        serde_json::from_str(&text[start..=end]).map_err(|e| format!("{}", e))
    };
    let mut manifest_json: serde_json::Value = match parse_manifest(&manifest_reply) {
        Ok(v) => v,
        // Malformed JSON is a shape failure, not a content failure: one corrective
        // round with the parser's complaint quoted back.
        Err(e) => {
            let retry = format!(
                "{}\nYour reply was not valid JSON ({}). Reply again with the same object, valid JSON this time, nothing else: no prose, no trailing commas, every string quoted.",
                manifest_user, e
            );
            let again = llm
                .chat(instructions, &retry, &format!("gen {}", id), "manifest format retry")
                .map_err(|e| format!("manifest format retry: {}", e))?;
            parse_manifest(&again).map_err(|e| format!("manifest JSON: {}", e))?
        }
    };
    // The manifest must agree with the artifact: a present test left undeclared, or a
    // declared programmatic test absent from the artifact, gets one corrective retry.
    // Rows still wrong after it fall back to llm in the validation below.
    let mut mismatches: Vec<String> = all_reqs
        .iter()
        .filter_map(|r| {
            let rid = r["id"].as_str().unwrap_or_default();
            let name = r["testName"].as_str().unwrap_or_default();
            let row = manifest_json["tests"]
                .as_array()
                .and_then(|a| a.iter().find(|t| t["requirement"].as_str() == Some(rid)).cloned());
            // The suggested name is a suggestion: a test the generator named its own
            // way still counts, as long as the name it declared is in the artifact
            // (docs/consumers/gen.md#file-ownership-and-conventions).
            let declared_name = row.as_ref().and_then(|t| t["name"].as_str()).unwrap_or("").trim().to_string();
            let present = (!name.is_empty() && tests_code.contains(name))
                || (!declared_name.is_empty() && tests_code.contains(&declared_name));
            let programmatic = row.as_ref().map(|t| t["kind"].as_str() == Some("programmatic")).unwrap_or(false);
            let run = row.as_ref().and_then(|t| t["run"].as_str()).unwrap_or("").trim().to_string();
            let selector = if !declared_name.is_empty() && tests_code.contains(&declared_name) { &declared_name } else { name };
            let has_run = !run.is_empty() && runs_the_test(&run, selector);
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
    // The entry must actually compose the parts. The harness cannot read the medium,
    // but it can see whether the entry names the files the parts live in; an entry
    // that re-implements a part or drops one does not
    // (docs/consumers/gen.md#the-build).
    if let Some(m) = medium.as_ref().filter(|m| m.is_built()) {
        let entry_path = task["buildEntry"]["path"]
            .as_str()
            .map(String::from)
            .or_else(|| entry_from_run(manifest_json["build"]["run"].as_str().unwrap_or("")));
        // What the entry would hold after this task: the version it returns, or the
        // one already on disk when it returns none.
        let entry_content = manifest_json["supportFiles"]
            .as_array()
            .and_then(|a| {
                a.iter().find(|f| {
                    Some(norm_rel(f["path"].as_str().unwrap_or_default())) == entry_path.as_ref().map(|p| norm_rel(p))
                })
            })
            .and_then(|f| f["content"].as_str())
            .map(String::from)
            // The package's copy was read before this task wrote anything, so the
            // file on disk is the fresher answer when the manifest returns none.
            .or_else(|| {
                entry_path
                    .as_ref()
                    .and_then(|p| std::fs::read_to_string(gs.deliverable.join(p)).ok())
                    .or_else(|| task["buildEntry"]["content"].as_str().map(String::from))
            });
        if let (Some(entry), Some(content)) = (&entry_path, &entry_content) {
            // Every part: this task's product file plus the ones other tasks wrote.
            let mut parts: Vec<String> = vec![product_rel.clone()];
            parts.extend(
                task["generatedFiles"]
                    .as_object()
                    .map(|o| {
                        o.values()
                            .flat_map(|v| v["files"].as_array().cloned().unwrap_or_default())
                            .filter_map(|f| f.as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            );
            let missing: Vec<String> = parts
                .into_iter()
                .map(|p| norm_rel(&p))
                .filter(|p| p != &norm_rel(entry) && !p.contains("test"))
                .filter(|p| {
                    let stem = std::path::Path::new(p).file_stem().map(|s| s.to_string_lossy().to_string());
                    !stem.map(|st| content.contains(&st)).unwrap_or(true)
                })
                .collect();
            if !missing.is_empty() {
                mismatches.push(format!(
                    "- the build entry `{}` does not name these parts, so `{}` will not carry them: {}. Return `{}` in supportFiles, importing each one and calling into it, and keep everything it already composes; never re-implement a part inside the entry",
                    entry,
                    m.artifact,
                    missing.join(", "),
                    entry
                ));
            }
        }
    }

    // A built medium with no build recorded has nothing to verify, so the missing
    // command joins the same corrective retry the artifact mismatches use
    // (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
    if let Some(m) = medium.as_ref().filter(|m| m.is_built()) {
        let recorded = !task["build"].is_null();
        let given = !manifest_json["build"]["run"].as_str().unwrap_or("").trim().is_empty();
        if !recorded && !given {
            mismatches.push(format!(
                "- `build` is missing: this deliverable is {}, so the manifest must carry the command that produces `{}` (`run`, `cwd`, `produces`)",
                m.form, m.artifact
            ));
        }
    }
    if !mismatches.is_empty() {
        // The previous answer goes back with the complaint: a correction changes what
        // the complaint names and nothing else
        // (docs/consumers/gen.md#file-ownership-and-conventions).
        let retry = format!(
            "{}\nYour answer was:\n{}\n\nIt has these problems:\n{}\nReturn the corrected, complete JSON manifest object, same schema, no prose. Change only what the problems name; keep every other field exactly as you gave it, supportFiles and build included.",
            manifest_user,
            serde_json::to_string_pretty(&manifest_json).unwrap_or_default(),
            mismatches.join("\n")
        );
        if let Ok(reply2) = llm.chat(instructions, &retry, &format!("gen {}", id), "manifest retry") {
            if let Ok(mut v) = parse_manifest(&reply2) {
                // Merge over the first answer: a retry that forgot a field keeps it.
                for key in ["supportFiles", "build"] {
                    let dropped = match &v[key] {
                        serde_json::Value::Null => true,
                        serde_json::Value::Array(a) => a.is_empty(),
                        serde_json::Value::Object(o) => o.is_empty(),
                        _ => false,
                    };
                    if dropped && !manifest_json[key].is_null() {
                        v[key] = manifest_json[key].clone();
                    }
                }
                manifest_json = v;
            }
        }
    }
    if let Some(support) = manifest_json["supportFiles"].as_array() {
        for f in support {
            let (Some(path), Some(content)) = (f["path"].as_str(), f["content"].as_str()) else { continue };
            if path.starts_with('/') || path.contains("..") {
                continue;
            }
            // A support file belongs to the deliverable, which is exactly why it must
            // not land on a file some entity owns: this task's own product and tests
            // included. Ownership is what stops one file from eating another
            // (docs/consumers/gen.md#file-ownership-and-conventions).
            if owner_of(path).is_some() || path == product_rel || path == tests_rel {
                continue;
            }
            let p = gs.deliverable.join(path);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            snapshot_baseline(&store.out, gs, path, &mut baselined);
            std::fs::write(&p, content).map_err(|e| e.to_string())?;
            support_files.push(path.to_string());
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
        // The name the row is selected by: the suggestion when the artifact carries
        // it, otherwise whatever the generator named its test, as long as the artifact
        // carries that (docs/consumers/gen.md#file-ownership-and-conventions).
        let declared_name = row.and_then(|t| t["name"].as_str()).unwrap_or("").trim();
        let selector = if !name.is_empty() && tests_code.contains(name) {
            name
        } else if !declared_name.is_empty() && tests_code.contains(declared_name) {
            declared_name
        } else {
            ""
        };
        let programmatic = row
            .map(|t| {
                let run = t["run"].as_str().unwrap_or("").trim();
                t["kind"].as_str() == Some("programmatic")
                    && !run.is_empty()
                    && !selector.is_empty()
                    && runs_the_test(run, selector)
            })
            .unwrap_or(false);
        if programmatic {
            let t = row.unwrap();
            tests_manifest.push(serde_json::json!({
                "requirement": rid, "kind": "programmatic",
                "label": t["label"].as_str().unwrap_or("test"),
                "artifact": tests_rel, "name": selector,
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
    // The extra files a step wrote are sorted now that the manifest is known: the
    // deliverable keeps what the manifest calls support or runs as the build entry,
    // the entity keeps the rest (docs/consumers/gen.md#file-ownership-and-conventions).
    let declared_support: Vec<String> = manifest_json["supportFiles"]
        .as_array()
        .map(|a| a.iter().filter_map(|f| f["path"].as_str().map(norm_rel)).collect())
        .unwrap_or_default();
    let build_entry_path = task["buildEntry"]["path"]
        .as_str()
        .map(norm_rel)
        .or_else(|| entry_from_run(manifest_json["build"]["run"].as_str().unwrap_or("")).map(|p| norm_rel(&p)));
    for p in extra_written {
        let n = norm_rel(&p);
        let deliverable_wide =
            declared_support.iter().any(|d| *d == n) || build_entry_path.as_deref() == Some(n.as_str());
        if deliverable_wide {
            support_files.push(p);
        } else if !files.contains(&p) {
            files.push(p);
        }
    }
    let mut manifest = serde_json::json!({"files": files, "tests": tests_manifest, "supportFiles": support_files});
    // The build the manifest step returned rides along: it is the deliverable's, not
    // this row's, and mark records it once (docs/consumers/gen.md#the-build).
    if !manifest_json["build"]["run"].as_str().unwrap_or("").trim().is_empty() {
        manifest["build"] = manifest_json["build"].clone();
    }
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
