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
    // Globs scoping which deliverable files count as implementation for the unclaimed
    // report and decompilation. Empty: everything minus the standard exclusions.
    pub code: Vec<String>,
}

impl GenSettings {
    pub fn resolve(proj: &crate::project::Project) -> GenSettings {
        let deliverable = match &proj.gen_deliverable {
            Some(d) => proj.root.join(d),
            // Default: the project root itself. The docs glob keeps the source tree
            // out of the product's way (docs/compiler/project-settings.md#generation).
            None => proj.root.clone(),
        };
        GenSettings {
            deliverable,
            worker: proj.gen_worker.clone().unwrap_or_else(|| "agentic".into()),
            code: proj.gen_code.clone(),
        }
    }

    // Placeholder for sessions with no project (benchmark cases). Gen tools are absent
    // from those toolsets, so the path is never read.
    pub fn from_out(out: &Path) -> GenSettings {
        GenSettings {
            deliverable: out.join("gen").join("deliverable"),
            worker: "agentic".into(),
            code: Vec::new(),
        }
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
    // Rows recorded over an open error diagnostic: requirement id to the diagnostic
    // ids open at record time. A record, never a verdict; a later record of the row
    // under a clean graph clears its entry.
    // Mirrors docs/consumers/gen.md#rows-recorded-over-an-open-contradiction.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contradicted: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub entities: BTreeMap<String, EntityGen>,
    #[serde(default)]
    pub requirements: BTreeMap<String, ReqRow>,
}

impl Ledger {
    // Flag or clear one row against the diagnostics open right now. Called by every
    // record of a row, so the map never outlives the dispute.
    // Mirrors docs/consumers/gen.md#rows-recorded-over-an-open-contradiction.
    pub fn record_contradicted(&mut self, store: &Store, rid: &str) -> Vec<String> {
        let ids: Vec<String> = open_errors_on(store, rid)
            .iter()
            .filter_map(|d| d["id"].as_str().map(String::from))
            .collect();
        if ids.is_empty() {
            self.contradicted.remove(rid);
        } else {
            self.contradicted.insert(rid.to_string(), ids.clone());
        }
        ids
    }
}

// The open error diagnostics naming a requirement, suppressed excluded: what a
// package says before a session writes against a disputed statement. Sorted by id.
// Mirrors docs/consumers/gen.md#rows-recorded-over-an-open-contradiction.
pub fn open_errors_on(store: &Store, rid: &str) -> Vec<Value> {
    let rid = store.resolve_id(rid);
    let mut out: Vec<(&String, &crate::model::Diagnostic)> = store
        .graph
        .diagnostics
        .iter()
        .filter(|(_, d)| {
            d.lifecycle == "open"
                && d.severity == "error"
                && d.triage.as_deref() != Some("suppressed")
                && d.subjects.iter().any(|s| store.resolve_id(s) == rid)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(b.0));
    out.iter()
        .map(|(id, d)| json!({"id": id, "rule": d.rule, "message": d.message}))
        .collect()
}

// The tool family a word of a run command or a toolchain description names. Unknown
// words name none, so a divergence is only ever declared between two known families.
// Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
fn tool_family(word: &str) -> Option<&'static str> {
    let w = word
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '+' && c != '#')
        .to_lowercase();
    let w = w.rsplit('/').next().unwrap_or(&w);
    Some(match w {
        "cargo" | "rustc" | "rust" => "rust",
        "python" | "python3" | "pytest" | "pip" | "pip3" | "poetry" | "uv" => "python",
        "node" | "npm" | "npx" | "yarn" | "pnpm" | "jest" | "vitest" | "mocha" | "tsc" | "deno"
        | "bun" | "javascript" | "typescript" => "node",
        "go" | "golang" => "go",
        "mvn" | "maven" | "gradle" | "java" | "javac" | "kotlin" => "jvm",
        "dotnet" | "csharp" | "c#" => "dotnet",
        "ruby" | "rspec" | "bundle" | "gem" => "ruby",
        "swift" => "swift",
        "gcc" | "clang" | "g++" | "make" | "cmake" | "ctest" => "c",
        "php" | "phpunit" | "composer" => "php",
        _ => return None,
    })
}

// The decided medium's toolchain against the recorded programmatic run commands.
// Some when both name a tool family and share none: the medium diverged from what
// the sessions actually wrote. None when either side names no known family.
// Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
pub fn medium_divergence(ledger: &Ledger) -> Option<String> {
    let medium = ledger.medium.as_ref()?;
    let mut toolchain: Vec<&str> = medium
        .toolchain
        .split_whitespace()
        .filter_map(tool_family)
        .collect();
    toolchain.sort();
    toolchain.dedup();
    if toolchain.is_empty() {
        return None;
    }
    // One family per command: the first known tool word, so `sh -c 'cargo test'`
    // reads as Rust and `python -m pytest` as Python.
    let mut commands: Vec<(&'static str, &str)> = ledger
        .requirements
        .values()
        .filter(|r| r.test.kind == "programmatic")
        .filter_map(|r| {
            r.test
                .run
                .split_whitespace()
                .find_map(tool_family)
                .map(|f| (f, r.test.run.as_str()))
        })
        .collect();
    commands.sort();
    commands.dedup();
    if commands.is_empty() || commands.iter().any(|(f, _)| toolchain.contains(f)) {
        return None;
    }
    let mut families: Vec<&str> = commands.iter().map(|(f, _)| *f).collect();
    families.dedup();
    let sample: Vec<String> = commands
        .iter()
        .take(3)
        .map(|(_, run)| format!("`{}`", run))
        .collect();
    Some(format!(
        "the decided medium's toolchain `{}` names {}, while the recorded run commands name {} ({})",
        medium.toolchain,
        toolchain.join(", "),
        families.join(", "),
        sample.join(", ")
    ))
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
            format!(
                "{} written directly ({}). The files you write ARE the deliverable.",
                self.form, self.toolchain
            )
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
    #[serde(
        default,
        rename = "lastRun",
        alias = "last_run",
        skip_serializing_if = "Option::is_none"
    )]
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
pub fn run_build(
    out: &Path,
    gs: &GenSettings,
    trace: &crate::session::Trace,
    label: &str,
) -> Result<(), String> {
    let Some(b) = Ledger::load(out).build.clone() else {
        return Ok(());
    };
    let cwd = gs.deliverable.join(&b.cwd);
    trace.line(label, &format!("build: {} (in {})", b.run, cwd.display()));
    let done = std::process::Command::new("sh")
        .arg("-c")
        .arg(&b.run)
        .current_dir(&cwd)
        .output()
        .map_err(|e| {
            format!(
                "build `{}` could not start in {}: {}",
                b.run,
                cwd.display(),
                e
            )
        })?;
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
    let missing: Vec<&String> = b
        .produces
        .iter()
        .filter(|p| !gs.deliverable.join(p).exists())
        .collect();
    if !missing.is_empty() {
        let names = missing
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        record_build_run(
            out,
            false,
            &format!("exited 0 but did not produce {}", names),
        );
        return Err(format!(
            "build `{}` exited 0 but did not produce {}",
            b.run, names
        ));
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
    // The unattached remainder, measured at record time: how much of what this
    // entity's generation produced no requirement claims.
    // Mirrors docs/consumers/gen.md#the-unattached-remainder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unattached: Option<Unattached>,
}

// Generated mass attached to no requirement: owned files no row names, significant
// lines outside every site's run, and their share of the entity's significant lines.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Unattached {
    pub files: u64,
    pub lines: u64,
    pub ratio: f64,
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

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
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
    // `(?:req:)+` folds a doubled prefix (`req:req:orders-1`, a model pasting the
    // full id after a template's literal `req:`) into one clean site.
    let re = regex::Regex::new(r"(?:req:)+([A-Za-z0-9][A-Za-z0-9_-]*)\s+hash:([0-9a-fA-F]{4,64})")
        .unwrap();
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
                sites.push(RawSite {
                    rid,
                    line: lineno,
                    head: line.to_string(),
                });
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
        // Criteria live in the out directory when the built-in worker wrote them; an
        // external worker writes them where it writes everything else, under the
        // deliverable. The recorded path resolves against both, out first, so neither
        // home reads as a gone artifact.
        let in_out = out.join("gen").join(&test.artifact);
        if in_out.exists() {
            return in_out;
        }
        let in_deliverable = gs.deliverable.join(&test.artifact);
        if in_deliverable.exists() {
            return in_deliverable;
        }
        in_out
    } else {
        gs.deliverable.join(&test.artifact)
    }
}

pub fn hash_file(path: &Path) -> String {
    std::fs::read(path)
        .map(|b| hash_hex(&String::from_utf8_lossy(&b)))
        .unwrap_or_default()
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

// A deliverable-relative path as the ledger records it: forward slashes, no leading
// `./`, no empty or `.` segments. An absolute path, or one that climbs out with
// `..`, is rejected naming it: nothing the ledger records, strips, hashes, or
// removes may reach outside the deliverable.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
pub fn confine_rel(path: &str) -> Result<String, String> {
    let raw = path.trim().replace('\\', "/");
    let drive = raw.as_bytes().get(1) == Some(&b':')
        && raw
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false);
    if raw.starts_with('/') || drive {
        return Err(format!(
            "`{}` is an absolute path; every recorded path is relative to the deliverable directory",
            path.trim()
        ));
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in raw.split('/') {
        match seg {
            "" | "." => continue,
            ".." => {
                return Err(format!(
                    "`{}` climbs out of the deliverable with `..`; every recorded path stays under it",
                    path.trim()
                ))
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return Err("a path is required, and `.` names the deliverable itself, not a file".into());
    }
    Ok(parts.join("/"))
}

// A working directory the same way, where `.` (or nothing) means the deliverable.
pub fn confine_cwd(cwd: &str) -> Result<String, String> {
    let c = cwd.trim();
    if c.is_empty() || c == "." || c == "./" {
        return Ok(".".into());
    }
    confine_rel(c)
}

// Every path a list names, confined, deduplicated in first-seen order.
fn confine_list(paths: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for p in paths {
        out.push(confine_rel(p)?);
    }
    Ok(dedup_keep_order(out))
}

fn dedup_keep_order(files: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    files
        .into_iter()
        .filter(|f| seen.insert(f.clone()))
        .collect()
}

pub fn slug_of(id: &str) -> String {
    id.strip_prefix("ent:").unwrap_or(id).to_string()
}

pub fn req_slug(id: &str) -> String {
    id.strip_prefix("req:").unwrap_or(id).to_string()
}

// The suggested test name: requirement id + hash prefix, sanitized. A reworded
// requirement mechanically breaks the recorded run filter.
pub fn test_name(rid: &str, statement: &str) -> String {
    let sanitized: String = req_slug(rid)
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("req_{}_{}", sanitized, &hash_hex(statement)[..8])
}

pub fn reqs_of_sorted(store: &Store, id: &str) -> Vec<String> {
    let mut v = store.requirements_referencing(id);
    v.sort();
    v
}

// The facts generation consumes, folded into one hash: name, definition,
// stereotype, attributes, and every referencing statement with its edges. Any of
// them changing flips the entity to generation work.
// Mirrors docs/consumers/gen.md#the-ledger.
pub fn fact_hash(store: &Store, id: &str) -> String {
    let e = &store.graph.entities[id];
    let mut facts = format!(
        "{}|{}|{}|",
        e.name,
        e.definition.as_deref().unwrap_or(""),
        e.stereotype.as_deref().unwrap_or("")
    );
    for a in &e.attributes {
        facts.push_str(&format!(
            "{}:{}:{}|",
            a.name,
            a.r#type.as_deref().unwrap_or(""),
            a.value.as_deref().unwrap_or("")
        ));
    }
    for rid in reqs_of_sorted(store, id) {
        if let Some(r) = store.graph.requirements.get(&rid) {
            facts.push_str(&r.statement);
            for edge in &r.edges {
                facts.push_str(&format!(
                    "{}>{}:{}:{}",
                    edge.a,
                    edge.b,
                    edge.rel_type.as_deref().unwrap_or(""),
                    edge.cardinality.as_deref().unwrap_or("")
                ));
            }
            facts.push('|');
        }
    }
    hash_hex(&facts)
}

// The deliverable's current file listing, relative paths, two levels deep, capped:
// enough to pin a medium (a Cargo.toml, a src/ tree) without flooding the prompt.
fn list_existing_files(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>, depth: usize) {
        if depth > 2 || out.len() >= 40 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<std::fs::DirEntry> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if out.len() >= 40 {
                return;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out, depth + 1);
            } else if let Ok(rel) = p.strip_prefix(root) {
                out.push(rel.to_string_lossy().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out, 0);
    out
}

// Decide what the deliverable is made of, once, from the statements that say so. The
// answer is recorded in the ledger and stated as a fact to every later task, so no
// per-entity task has to work it out again.
// Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
pub fn decide_medium(
    store: &Store,
    runner: &crate::acp::runner::AcpRunner,
    deliverable: &Path,
) -> Result<Medium, String> {
    // Every statement in the graph, capped: the medium is stated somewhere in the
    // documents, and which statement says it is exactly what the model must find.
    let mut statements: Vec<String> = store
        .graph
        .requirements
        .iter()
        .map(|(rid, r)| format!("- {}: {}", rid, r.statement))
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
    let entities: Vec<&str> = store
        .graph
        .entities
        .values()
        .map(|e| e.name.as_str())
        .take(40)
        .collect();
    // An existing deliverable pins the medium: a planted fixture or an earlier run
    // already chose the language and toolchain, and a guess from CLI-flavored
    // statements must not override it. "From the statements alone" applies only to
    // an empty deliverable.
    // Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
    let existing = list_existing_files(deliverable);
    let mut tree = if existing.is_empty() {
        "The deliverable directory is empty: decide from the statements alone.".to_string()
    } else {
        format!(
            "The deliverable directory already holds these files:\n{}\nAn existing tree pins the medium: decide the form and toolchain these files already use (their language, build manifest, and test layout), never a different one.",
            existing.join("\n")
        )
    };
    // Run commands recorded before any entity generated (bound tests) pin the
    // toolchain the same way an existing tree does: a re-decision after a divergence
    // must land on what the sessions actually wrote.
    // Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
    let mut commands: Vec<String> = Ledger::load(&store.out)
        .requirements
        .values()
        .filter(|r| r.test.kind == "programmatic" && !r.test.run.trim().is_empty())
        .map(|r| r.test.run.clone())
        .collect();
    commands.sort();
    commands.dedup();
    if !commands.is_empty() {
        tree.push_str(&format!(
            "\n\nThe ledger already records these test run commands:\n{}\nRecorded commands pin the toolchain: decide the toolchain these commands run under, never a different one.",
            commands.iter().take(10).map(|c| format!("- {}", c)).collect::<Vec<_>>().join("\n")
        ));
    }
    let system =
        "You decide what a deliverable is made of. Answer with one JSON object and nothing else.";
    let user = format!(
        "These statements are the whole specification of one deliverable.\n\n{}\n\nEntities: {}\n\n{}\n\n\
         Decide the deliverable's medium.\n\
         - `form`: what the deliverable is, in a few words (e.g. `Rust library`, `Microsoft PowerPoint deck`, `printed book`).\n\
         - `produced`: `written` when the files a generator writes ARE the deliverable (source code, a manuscript, a configuration). \
         `built` when the medium is a format a tool must produce (a slide deck, a PDF, an image, a compiled binary): the files are the source, and a command turns them into the artifact.\n\
         - `toolchain`: what writes or builds it (e.g. `rustc and cargo test`, `python3 with python-pptx`). Name a library that can actually emit the format.\n\
         - `artifact`: for `built` only, the file the build produces, relative to the deliverable directory (e.g. `jazyk.pptx`). Empty for `written`.\n\n\
         Reply with exactly: {{\"form\": \"...\", \"produced\": \"written\"|\"built\", \"toolchain\": \"...\", \"artifact\": \"...\"}}",
        body,
        entities.join(", "),
        tree
    );
    let mut last = String::new();
    for attempt in 0..2 {
        let ask = if attempt == 0 {
            user.clone()
        } else {
            format!(
                "{}\n\nYour previous answer was rejected: {}. Reply with the JSON object only.",
                user, last
            )
        };
        let reply = runner.ask(
            system,
            &ask,
            "gen medium",
            if attempt == 0 {
                "decide"
            } else {
                "decide retry"
            },
        )?;
        let raw = crate::llm::extract_json_object(&reply).unwrap_or(reply);
        let v: Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                last = format!("not JSON ({})", e);
                continue;
            }
        };
        let produced = v["produced"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        if produced != "written" && produced != "built" {
            last = format!("`produced` was `{}`, not `written` or `built`", produced);
            continue;
        }
        let artifact = v["artifact"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .trim_start_matches("./")
            .to_string();
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
            toolchain: v["toolchain"]
                .as_str()
                .unwrap_or_default()
                .trim()
                .to_string(),
            artifact,
        });
    }
    Err(format!(
        "could not decide the deliverable's medium: {}",
        last
    ))
}

// The generation contract, identical for every worker. It never names a medium: what
// the deliverable is (a language, a format, a genre) is a fact the documents state,
// carried by the context pack.
pub fn instructions() -> String {
    include_str!("../../docs/compiler/goals/prompts/generate-contract.md")
        .replace("{GROUP}", &GROUP.to_string())
}

// The change diff for one entity versus the ledger.
fn change_diff(ledger: &Ledger, slug: &str, current: &[String]) -> (String, Vec<String>) {
    match ledger.entities.get(slug) {
        None => (
            "new".to_string(),
            current.iter().map(|r| format!("{} (added)", r)).collect(),
        ),
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
                changed.push(
                    "(reworded: same requirement set, changed statements or definition)"
                        .to_string(),
                );
            }
            ("changed".to_string(), changed)
        }
    }
}

// Entities that are generation work, the first reason that holds naming it: the
// entry's facts moved (`changed`), a recorded file is gone from the deliverable
// (`files-gone`), or a bound requirement is unimplemented (`unimplemented-bindings`:
// binding classified it as new functionality and its test is the acceptance gate).
// An entity with no entry is work through unimplemented rows only, so adopted code
// whose rows all read verified is never generated over.
// Mirrors docs/compiler/goals/generate.md#created-when.
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
        let unimplemented: Vec<&String> = rids
            .iter()
            .filter(|rid| {
                ledger
                    .requirements
                    .get(*rid)
                    .map(|row| crate::verify::status_of(store, rid, row, gs).0 == "unimplemented")
                    .unwrap_or(false)
            })
            .collect();
        let (reason, changed) = match ledger.entities.get(&slug) {
            Some(e) if e.fact_hash != hash => {
                let (r, c) = change_diff(&ledger, &slug, &rids);
                (r, json!(c))
            }
            Some(e) => {
                let gone: Vec<&String> = e
                    .files
                    .iter()
                    .filter(|f| !gs.deliverable.join(f).exists())
                    .collect();
                if e.files.is_empty() || !gone.is_empty() {
                    ("files-gone".to_string(), json!(gone))
                } else if !unimplemented.is_empty() {
                    ("unimplemented-bindings".to_string(), json!(unimplemented))
                } else {
                    continue;
                }
            }
            None if !unimplemented.is_empty() => {
                ("unimplemented-bindings".to_string(), json!(unimplemented))
            }
            None => continue,
        };
        out.push(json!({
            "entity": id,
            "reason": reason,
            "changed": changed,
        }));
    }
    out
}

// Why `jazyk gen` leaves an entity alone, for the person reading the trace: the
// entity is current, a bind is still owed on it, or no row says generate.
// Mirrors docs/consumers/gen.md#incremental-regeneration.
pub fn skip_reason(store: &Store, gs: &GenSettings, id: &str) -> String {
    let owed = crate::bind::pending(store, gs)
        .iter()
        .filter(|p| p["entity"] == id)
        .count();
    if owed > 0 {
        return format!(
            "{} requirement(s) still owe a bind; binding classifies before generation",
            owed
        );
    }
    let ledger = Ledger::load(&store.out);
    if ledger.entities.contains_key(&slug_of(id)) {
        "unchanged".to_string()
    } else {
        "no ledger entry and no bound row reads unimplemented: its rows are verified or failing, so there is nothing to generate (`--force` generates it anyway)".to_string()
    }
}

// The full package a worker needs for one task.
// The file the build command runs, with its current content. The command names it as
// an argument, so the entry is the first token that exists under the deliverable.
// Mirrors docs/consumers/gen.md#the-build.
fn build_entry(ledger: &Ledger, gs: &GenSettings) -> Value {
    let Some(b) = &ledger.build else {
        return Value::Null;
    };
    let dir = gs.deliverable.join(&b.cwd);
    for token in b.run.split_whitespace() {
        let token = token.trim_matches(|c| c == '"' || c == '\'');
        if token.starts_with('-') || token.is_empty() {
            continue;
        }
        let candidate = dir.join(token);
        if candidate.is_file() {
            let rel = norm_rel(
                &pathdiff(&candidate, &gs.deliverable).unwrap_or_else(|| token.to_string()),
            );
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
        .find(|t| {
            !t.starts_with('-')
                && t.contains('.')
                && !t.ends_with("python")
                && !t.ends_with("python3")
        })
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
    path.strip_prefix(base)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
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
                            "statement": r.statement,
                            "quote": r.source.as_ref().map(|s| s.quote.clone()).unwrap_or_default(),
                            "provenance": crate::session::provenance_line(r),
                            "hash": hash_hex(&r.statement),
                            "testName": test_name(rid, &r.statement),
                            "criteriaPath": format!("criteria/req-{}.md", req_slug(rid)),
                            // The statement is disputed while these stand; the session
                            // writes against one side of an open question and must
                            // know it (docs/consumers/gen.md#rows-recorded-over-an-open-contradiction).
                            "openDiagnostics": open_errors_on(store, rid),
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
                rel.strongest(),
                rel.members
                    .iter()
                    .filter(|m| *m != id)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
        .collect();
    // The goal's loaded set: the entity in full with its neighbors as stubs.
    // Mirrors docs/consumers/gen.md.
    let pack = crate::context::ledger_context(store, id);
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
                .filter_map(|rid| {
                    store
                        .graph
                        .requirements
                        .get(rid)
                        .map(|r| r.statement.clone())
                })
                .collect();
            // Under a built medium the entry this task rewrites has to call into
            // these files, so their content travels with them: a path and a
            // statement do not say what a part is called
            // (docs/consumers/gen.md#the-build).
            let show = ledger
                .medium
                .as_ref()
                .map(|m| m.is_built())
                .unwrap_or(false);
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
            (
                k,
                json!({"files": v.files, "holds": holds, "contents": contents}),
            )
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
        // The requirements under an open error diagnostic, and the medium's standing
        // divergence from the recorded commands, so neither is news at record time.
        "contradicted": rids
            .iter()
            .filter(|rid| !open_errors_on(store, rid).is_empty())
            .cloned()
            .collect::<Vec<String>>(),
        "mediumWarning": medium_divergence(&ledger),
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
        // The tests binding already wrote: they define the interface, and the product
        // conforms to them. A task that cannot make one pass without changing it
        // reports that; the repair is a re-bind
        // (docs/consumers/bind.md#generation-makes-bound-tests-pass).
        "boundTests": rids
            .iter()
            .filter_map(|rid| {
                ledger.requirements.get(rid).map(|row| {
                    let (status, _) = crate::verify::status_of(store, rid, row, gs);
                    json!({
                        "requirement": rid,
                        "status": status,
                        "test": {"kind": row.test.kind, "artifact": row.test.artifact,
                                 "name": row.test.name, "run": row.test.run, "cwd": row.test.cwd},
                    })
                })
            })
            .collect::<Vec<Value>>(),
    }))
}

// Record a task done. The manifest binds the worker's files to the graph and seeds the
// verification rows. Mirrors docs/compiler/tools.md#generation-tools (gen_mark).
// Empty means absent (docs/compiler/tools.md#validation-and-errors), mirrored here
// for manifest rows: a row whose fields are all empty is a filled-in blank from a
// schema-filling model, not data.
fn hollow(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(a) => a.iter().all(hollow),
        Value::Object(o) => o.values().all(hollow),
        _ => false,
    }
}

pub fn mark(
    store: &Store,
    id: &str,
    fact_hash_seen: Option<&str>,
    manifest: &Value,
    gs: &GenSettings,
) -> Result<Value, String> {
    if !store.graph.entities.contains_key(id) {
        return Err(format!("unknown entity `{}`", id));
    }
    let slug = slug_of(id);
    // Every path the manifest names is confined before any side effect: a path that
    // escapes the deliverable is rejected naming it, and file lists are sets
    // (docs/consumers/gen.md#file-ownership-and-conventions).
    let str_list = |v: &Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        x.as_str()
                            .map(String::from)
                            .or_else(|| x["path"].as_str().map(String::from))
                            .filter(|s| !s.trim().is_empty())
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let files: Vec<String> =
        confine_list(&str_list(&manifest["files"])).map_err(|e| format!("files: {}", e))?;
    // Support files are the deliverable's, recorded once and rewritable by any later
    // task (docs/consumers/gen.md#file-ownership-and-conventions).
    let support: Vec<String> = confine_list(&str_list(&manifest["supportFiles"]))
        .map_err(|e| format!("supportFiles: {}", e))?;
    let build_produces: Vec<String> = confine_list(&str_list(&manifest["build"]["produces"]))
        .map_err(|e| format!("build.produces: {}", e))?;
    let build_cwd = confine_cwd(manifest["build"]["cwd"].as_str().unwrap_or("."))
        .map_err(|e| format!("build.cwd: {}", e))?;

    // Validate the invented choices before any side effect: the scope grades the
    // severity, and the tool layer stages one invented-choice diagnostic per entry
    // from this validated set. Mirrors docs/consumers/gen.md#invented-choices.
    let choices = parse_choices(store, manifest)?;

    // Validate the manifest's test rows before any side effect: a rejection must
    // leave the deliverable untouched, or the retry sees files already stripped. A
    // row's artifact must exist, and a programmatic artifact must carry the declared
    // name: the same shape gate record_binding applies.
    // Mirrors docs/compiler/goals/generate.md#gate.
    let mut rows: Vec<(String, TestRef, Vec<String>)> = Vec::new();
    if let Some(tests) = manifest["tests"].as_array() {
        for t in tests {
            // A row whose fields are all empty is a filled-in blank; drop it.
            if hollow(t) {
                continue;
            }
            let Some(rid) = t["requirement"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let rid = store.resolve_id(rid).to_string();
            let Some(r) = store.graph.requirements.get(&rid) else {
                return Err(format!("unknown requirement `{}` in manifest", rid));
            };
            let kind = t["kind"].as_str().unwrap_or("programmatic").trim().to_string();
            if kind != "programmatic" && kind != "llm" {
                return Err(format!(
                    "test row for {} has kind `{}`; a test is `programmatic` (a command runs it) or `llm` (a judgment with a criteria file)",
                    rid, kind
                ));
            }
            let artifact_raw = t["artifact"].as_str().unwrap_or("").trim().to_string();
            let run = t["run"].as_str().unwrap_or("").trim().to_string();
            if kind == "programmatic" {
                if artifact_raw.is_empty() {
                    return Err(format!(
                        "test row for {} has an empty artifact; name the tests file the row's run command executes, or declare the row llm",
                        rid
                    ));
                }
                if run.is_empty() {
                    return Err(format!(
                        "test row for {} has an empty run command; record the exact command that runs only that test, or declare the row llm",
                        rid
                    ));
                }
            } else if artifact_raw.is_empty() {
                return Err(format!(
                    "test row for {} is llm but names no criteria file; write it (the package names its path under criteria/) and record that path as the artifact",
                    rid
                ));
            }
            let artifact = confine_rel(&artifact_raw)
                .map_err(|e| format!("test row for {}: artifact {}", rid, e))?;
            let test = TestRef {
                kind: kind.clone(),
                label: t["label"].as_str().unwrap_or("test").to_string(),
                artifact,
                name: t["name"]
                    .as_str()
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .unwrap_or(&test_name(&rid, &r.statement))
                    .to_string(),
                run,
                cwd: confine_cwd(t["cwd"].as_str().unwrap_or("."))
                    .map_err(|e| format!("test row for {}: cwd {}", rid, e))?,
            };
            let path = artifact_path(&store.out, gs, &test);
            if !path.is_file() {
                return Err(format!(
                    "test row for {} names artifact `{}` but no such file exists under the deliverable{}; write it before recording, or fix the path",
                    rid,
                    test.artifact,
                    if kind == "llm" { " or the out directory's gen/" } else { "" }
                ));
            }
            if kind == "programmatic"
                && !std::fs::read_to_string(&path)
                    .map(|c| c.contains(&test.name))
                    .unwrap_or(false)
            {
                return Err(format!(
                    "test row for {} declares test `{}` but `{}` does not contain that name; name the test as it is written in the file, or declare the row llm",
                    rid, test.name, test.artifact
                ));
            }
            let row_files: Vec<String> = match confine_list(&str_list(&t["files"]))
                .map_err(|e| format!("test row for {}: files {}", rid, e))?
            {
                v if v.is_empty() => files.clone(),
                v => v,
            };
            rows.push((rid, test, row_files));
        }
    }
    // Strip marker lines from the manifest files and collect the sites they anchor.
    // The marker is a wire format: the worker localizes while writing, the harness
    // records and cleans. Runs before hashing so every hash sees the stripped bytes.
    // Mirrors docs/consumers/gen.md#traceability.
    let mut sites_by_rid: BTreeMap<String, Vec<Site>> = BTreeMap::new();
    // A marker-like line the strip cannot parse (trailing text after the hash, a
    // mangled id) stays in the file and anchors nothing; name each one, so the row
    // never reads green over a broken wire format.
    // Mirrors docs/consumers/gen.md#traceability.
    let malformed_re =
        regex::Regex::new(r"req:[A-Za-z0-9][A-Za-z0-9_-]*\s+hash:[0-9a-fA-F]{4,}").unwrap();
    let mut marker_warnings: Vec<String> = Vec::new();
    for f in &files {
        let path = gs.deliverable.join(f);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let (clean, raw) = strip_markers(&text);
        for (i, line) in clean.lines().enumerate() {
            if marker_warnings.len() >= 20 {
                break;
            }
            if malformed_re.is_match(line) {
                marker_warnings.push(format!("{}:{}", f, i + 1));
            }
        }
        if raw.is_empty() {
            continue;
        }
        std::fs::write(&path, clean).map_err(|e| e.to_string())?;
        for s in raw {
            let rid = store.resolve_id(&s.rid).to_string();
            sites_by_rid.entry(rid).or_default().push(Site {
                file: f.clone(),
                line: s.line,
                head: s.head,
            });
        }
    }
    let mut ledger = Ledger::load(&store.out);
    // Deletion prunes the ledger: a requirement gone from the graph has no obligation
    // left, and no manifest can name it. Any record buries the dead rows, whatever
    // entity it marks. Mirrors docs/consumers/gen.md#deletion-prunes-the-ledger.
    let before = ledger.requirements.len();
    ledger
        .requirements
        .retain(|rid, _| store.graph.requirements.contains_key(store.resolve_id(rid)));
    let pruned = before - ledger.requirements.len();
    ledger
        .contradicted
        .retain(|rid, _| store.graph.requirements.contains_key(store.resolve_id(rid)));
    // One build per deliverable: the first task that needs one establishes it, later
    // tasks receive it in their package and reuse it.
    // Mirrors docs/consumers/gen.md#the-build.
    if ledger.build.is_none() {
        match manifest["build"]["run"]
            .as_str()
            .map(str::trim)
            .filter(|r| !r.is_empty())
        {
            Some(run) => {
                let mut produces = build_produces.clone();
                // A built medium names its artifact; a build that forgets to list it
                // would pass its own check while producing nothing.
                if let Some(m) = ledger.medium.as_ref().filter(|m| m.is_built()) {
                    if !produces
                        .iter()
                        .any(|p| p.trim_start_matches("./") == m.artifact)
                    {
                        produces.push(m.artifact.clone());
                    }
                }
                ledger.build = Some(Build {
                    run: run.to_string(),
                    cwd: build_cwd.clone(),
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
    // The previous record's file set, before it is overwritten: what this manifest
    // omits is removed below (docs/consumers/gen.md#incremental-regeneration).
    let prev_files: Vec<String> = ledger
        .entities
        .get(&slug)
        .map(|e| e.files.clone())
        .unwrap_or_default();
    ledger.entities.insert(
        slug.clone(),
        EntityGen {
            fact_hash: fact_hash_seen
                .map(String::from)
                .unwrap_or_else(|| fact_hash(store, id)),
            requirements: reqs_of_sorted(store, id),
            files: files.clone(),
            unattached: None,
        },
    );
    let mut seeded = 0;
    // Rows landing over an open error diagnostic: named in the reply and kept in
    // the ledger, so generation never runs green over a contradiction silently.
    // Mirrors docs/consumers/gen.md#rows-recorded-over-an-open-contradiction.
    let mut contradicted: BTreeMap<String, Vec<String>> = BTreeMap::new();
    {
        for (rid, test, row_files) in rows {
            let r = &store.graph.requirements[&rid];
            let hashes = RowHashes {
                requirement: hash_hex(&r.statement),
                test: hash_file(&artifact_path(&store.out, gs, &test)),
                files: hash_files(gs, &row_files),
            };
            let owner = r
                .entities
                .first()
                .map(|e| store.resolve_id(e).to_string())
                .unwrap_or_else(|| id.to_string());
            // A re-record that changes nothing keeps its verdict: the hashes say the
            // requirement, the test, and the files are the very state the last run
            // judged, and discarding that judgment would punish an idempotent call.
            let prior = ledger
                .requirements
                .get(&rid)
                .filter(|row| row.hashes == hashes);
            let (verdict, last_run, exit_code, evidence) = match prior {
                Some(row) => (
                    row.verdict.clone(),
                    row.last_run.clone(),
                    row.exit_code,
                    row.evidence.clone(),
                ),
                None => ("none".into(), None, None, None),
            };
            ledger.requirements.insert(
                rid.clone(),
                ReqRow {
                    entity: owner,
                    files: row_files,
                    sites: sites_by_rid.remove(&rid).unwrap_or_default(),
                    test,
                    hashes,
                    verdict,
                    last_run,
                    exit_code,
                    evidence,
                },
            );
            let open = ledger.record_contradicted(store, &rid);
            if !open.is_empty() {
                contradicted.insert(rid.clone(), open);
            }
            seeded += 1;
        }
    }
    // A regeneration replaces the file set: files the previous record listed that
    // this manifest omits are removed, snapshotted first, unless something else
    // still claims them (another entity's record, the support list, a row's test
    // artifact, the build's output). Whichever worker records, the deliverable
    // never keeps a predecessor under an old name
    // (docs/consumers/gen.md#incremental-regeneration).
    let mut removed: Vec<String> = Vec::new();
    let mut baselined: std::collections::HashSet<String> = Default::default();
    for f in &prev_files {
        let claimed = files.contains(f)
            || ledger.support.contains(f)
            || ledger
                .entities
                .iter()
                .any(|(s, e)| s != &slug && e.files.contains(f))
            || ledger.requirements.values().any(|r| &r.test.artifact == f)
            || ledger
                .build
                .as_ref()
                .map(|b| b.produces.contains(f))
                .unwrap_or(false);
        if claimed || !gs.deliverable.join(f).is_file() {
            continue;
        }
        snapshot_baseline(&store.out, gs, f, &mut baselined);
        if std::fs::remove_file(gs.deliverable.join(f)).is_ok() {
            removed.push(f.clone());
        }
    }
    // The deliverable itself measures how much was invented: owned mass no
    // requirement claims, recorded on the entity's entry so the grade and the
    // measure read together. Mirrors docs/consumers/gen.md#the-unattached-remainder.
    let unattached = measure_unattached(&ledger, gs, &slug);
    if let Some(e) = ledger.entities.get_mut(&slug) {
        e.unattached = Some(unattached.clone());
    }
    ledger.save(&store.out);
    let mut reply = json!({
        "marked": id, "files": files.len(), "tests": seeded,
        "unattached": {"files": unattached.files, "lines": unattached.lines, "ratio": unattached.ratio},
    });
    if !removed.is_empty() {
        reply["removed"] = json!(removed);
        reply["removedNote"] = json!(
            "files the previous record listed and this manifest omits were removed from the deliverable (their last content is under deliverable-baseline/ in the out directory)"
        );
    }
    if !choices.is_empty() {
        reply["choices"] = json!(choices.len());
    }
    if pruned > 0 {
        reply["prunedRows"] = json!(pruned);
        reply["note"] = json!("pruned ledger row(s) whose requirement left the graph");
    }
    if !marker_warnings.is_empty() {
        reply["markerWarnings"] = json!(marker_warnings);
        reply["markerNote"] = json!(
            "marker-like lines the strip could not parse remain in these files and anchor no site; a marker is `req:<id> hash:<hash8>` alone on its line"
        );
    }
    if !contradicted.is_empty() {
        reply["contradicted"] = json!(contradicted);
        reply["contradictedNote"] = json!(
            "these rows were recorded while an open error diagnostic disputes their statement; the deliverable implements one side of an open question until it is resolved"
        );
    }
    // An entity is generated now, so the medium stands; the divergence is the record
    // (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
    if let Some(w) = medium_divergence(&ledger) {
        reply["mediumWarning"] = json!(w);
    }
    Ok(reply)
}

// The unattached remainder for one entity, over the files it owns: files no
// requirement row names, and significant lines outside every site's run. A site's
// run starts at its head line and ends before the next site in the same file, or at
// the end of the file, so the unattached lines of a file with sites are the ones
// before its first site; a file with none is unattached whole. Test artifacts are
// claimed by their rows and excluded, support files never enter an entity's list.
// Mirrors docs/consumers/gen.md#the-unattached-remainder.
pub fn measure_unattached(ledger: &Ledger, gs: &GenSettings, slug: &str) -> Unattached {
    let Some(e) = ledger.entities.get(slug) else {
        return Unattached::default();
    };
    let mut named: std::collections::BTreeSet<&str> = Default::default();
    let mut artifacts: std::collections::BTreeSet<&str> = Default::default();
    let mut first_site: BTreeMap<&str, usize> = BTreeMap::new();
    for row in ledger.requirements.values() {
        for f in &row.files {
            named.insert(f);
        }
        artifacts.insert(row.test.artifact.as_str());
        for s in &row.sites {
            let e = first_site.entry(s.file.as_str()).or_insert(s.line);
            *e = (*e).min(s.line);
        }
    }
    let files = e
        .files
        .iter()
        .filter(|f| !named.contains(f.as_str()) && !artifacts.contains(f.as_str()))
        .count() as u64;
    let (mut significant, mut lines) = (0u64, 0u64);
    for f in &e.files {
        if artifacts.contains(f.as_str()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(gs.deliverable.join(f)) else {
            continue;
        };
        let covered_from = first_site.get(f.as_str()).copied();
        for (i, line) in text.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || is_comment_line(t) {
                continue;
            }
            significant += 1;
            if covered_from.map(|start| i + 1 < start).unwrap_or(true) {
                lines += 1;
            }
        }
    }
    let ratio = if significant == 0 {
        0.0
    } else {
        ((lines as f64 / significant as f64) * 100.0).round() / 100.0
    };
    Unattached {
        files,
        lines,
        ratio,
    }
}

// A comment by leader alone: anchoring never parses the medium, and neither does
// this measure. Covers the common comment syntaxes; a shebang counts as a comment.
fn is_comment_line(trimmed: &str) -> bool {
    ["//", "#", "--", ";", "*", "/*", "*/", "<!--"]
        .iter()
        .any(|lead| trimmed.starts_with(lead))
}

// ---- invented choices ----
// Anything the deliverable needed that the documents do not state. The manifest
// carries the choices; the harness grades each by the scope of the invention and
// files one invented-choice diagnostic per entry.
// Mirrors docs/consumers/gen.md#invented-choices.

#[derive(Clone)]
pub struct Choice {
    pub choice: String,
    // product | behavior | detail.
    pub scope: String,
    pub reasoning: String,
    pub requirements: Vec<String>,
}

// The severity a scope grades: the invention of the product is an error, of an
// observable behavior a warning, of a cosmetic detail a suppressible info.
pub fn severity_for_scope(scope: &str) -> &'static str {
    match scope {
        "product" => "error",
        "behavior" => "warning",
        _ => "info",
    }
}

// Parse and validate the manifest's `choices`. Rejections name the fix; requirement
// references resolve through redirects and must exist.
pub fn parse_choices(store: &Store, manifest: &Value) -> Result<Vec<Choice>, String> {
    let Some(list) = manifest["choices"].as_array() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for c in list {
        // An all-empty entry is a filled-in blank, not an invented choice.
        if hollow(c) {
            continue;
        }
        let text = c["choice"]
            .as_str()
            .or_else(|| c["message"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if text.is_empty() {
            return Err(
                "a choices entry needs `choice`: the invented choice in one sentence".into(),
            );
        }
        let scope = c["scope"].as_str().unwrap_or("").trim().to_lowercase();
        if !matches!(scope.as_str(), "product" | "behavior" | "detail") {
            return Err(format!(
                "choice `{}` has scope `{}`; it must be `product`, `behavior`, or `detail`",
                crate::llm::truncate(&text, 60),
                scope
            ));
        }
        let reasoning = c["reasoning"]
            .as_str()
            .or_else(|| c["reason"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let mut raw: Vec<String> = c["requirements"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        x.as_str()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                    })
                    .collect()
            })
            .unwrap_or_default();
        if let Some(r) = c["requirement"].as_str() {
            raw.push(r.to_string());
        }
        let mut requirements = Vec::new();
        for r in raw {
            let rid = store.resolve_id(&r).to_string();
            if !store.graph.requirements.contains_key(&rid) {
                return Err(format!("unknown requirement `{}` in choices", r));
            }
            if !requirements.contains(&rid) {
                requirements.push(rid);
            }
        }
        out.push(Choice {
            choice: text,
            scope,
            reasoning,
            requirements,
        });
    }
    Ok(out)
}

// The mutations the tool layer stages for one entity's invented set: one
// invented-choice diagnostic per new choice (severity by scope, subjects the entity
// plus the requirements the choice fills in, the message naming the unattached
// remainder), and a resolve for every open one the new record omits, so a
// regeneration under better documents clears its own debt while a repeated choice
// keeps its diagnostic and its triage. The prompt proposes the sentence for the
// governing section (the requirement's source, else the entity's first mention) with
// an answer option to keep the choice unstated.
// Mirrors docs/consumers/gen.md#invented-choices.
pub fn choice_ops(
    store: &Store,
    id: &str,
    choices: &[Choice],
    unattached: Option<&Unattached>,
) -> Vec<crate::store::Op> {
    use crate::model::{Diagnostic, DiagnosticPrompt, PromptOption, SuggestedEdit};
    let mut ops = Vec::new();
    let open: Vec<(String, String)> = store
        .graph
        .diagnostics
        .iter()
        .filter(|(_, d)| {
            d.lifecycle == "open"
                && d.rule == "invented-choice"
                && d.subjects.iter().any(|s| store.resolve_id(s) == id)
        })
        .map(|(did, d)| (did.clone(), d.message.clone()))
        .collect();
    for (did, message) in &open {
        if !choices.iter().any(|c| message.contains(&c.choice)) {
            ops.push(crate::store::Op::ResolveDiagnostic {
                id: did.clone(),
                reason: "re-recorded without this choice".into(),
            });
        }
    }
    let measure = unattached
        .map(|u| {
            format!(
                " Unattached remainder on the entity: {} file(s), {} line(s), ratio {:.2}.",
                u.files, u.lines, u.ratio
            )
        })
        .unwrap_or_default();
    for c in choices {
        if open.iter().any(|(_, m)| m.contains(&c.choice)) {
            continue;
        }
        let mut subjects = vec![id.to_string()];
        subjects.extend(c.requirements.iter().cloned());
        let anchor = c
            .requirements
            .iter()
            .find_map(|rid| {
                store
                    .graph
                    .requirements
                    .get(rid)
                    .and_then(|r| r.source.as_ref())
                    .map(|s| (s.doc.clone(), s.section.clone()))
            })
            .or_else(|| {
                store
                    .graph
                    .entities
                    .get(id)
                    .and_then(|e| e.mentions.first())
                    .map(|m| (m.doc.clone(), m.section.clone()))
            });
        let mut options = Vec::new();
        if let Some((doc, section)) = anchor {
            options.push(PromptOption {
                label: format!("Insert into {} {}", doc, section),
                edit: Some(SuggestedEdit {
                    doc,
                    section,
                    old_text: String::new(),
                    new_text: c.choice.clone(),
                }),
                answer: None,
            });
        }
        options.push(PromptOption {
            label: "Keep the choice unstated".into(),
            edit: None,
            answer: Some("keep unstated".into()),
        });
        ops.push(crate::store::Op::ReportDiagnostic {
            id: String::new(),
            diagnostic: Diagnostic {
                rule: "invented-choice".into(),
                severity: severity_for_scope(&c.scope).into(),
                subjects,
                message: format!("Invented ({}): {}{}", c.scope, c.choice, measure),
                reasoning: (!c.reasoning.is_empty()).then(|| c.reasoning.clone()),
                lifecycle: "open".into(),
                triage: None,
                prompt: Some(DiagnosticPrompt {
                    question: "Should the documents state this choice?".into(),
                    options,
                    freeform: true,
                }),
                answer: None,
                created: None,
                updated: None,
            },
        });
    }
    ops
}

// ---- grouping by component ----
// Where the graph carries containment, a component and its subtree generate as one
// group; the goal stays per entity, the group is derived at batch time, never
// stored. Mirrors docs/consumers/gen.md#grouping-by-component.

// A system: a containment root with at least one child (the match that derives the
// default component view), or an entity the documents stereotype as one.
pub fn is_system(store: &Store, id: &str) -> bool {
    let Some(e) = store.graph.entities.get(id) else {
        return false;
    };
    if e.stereotype
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("system"))
        .unwrap_or(false)
    {
        return true;
    }
    e.parent.is_none()
        && store
            .graph
            .entities
            .values()
            .any(|c| c.parent.as_deref() == Some(id))
}

// The group root an entity generates under: the direct child of a system, the
// «service»-like tier of the containment tree. A system generates alone, and so
// does a parentless entity without children.
pub fn group_root(store: &Store, id: &str) -> String {
    if is_system(store, id) {
        return id.to_string();
    }
    let mut cur = id.to_string();
    // Bounded walk: the store refuses parent cycles, the bound keeps this total.
    for _ in 0..64 {
        let Some(parent) = store
            .graph
            .entities
            .get(&cur)
            .and_then(|e| e.parent.clone())
        else {
            return cur;
        };
        if is_system(store, &parent) {
            return cur;
        }
        cur = parent;
    }
    cur
}

// The component groups of an ordered target list: one group per root, groups in
// first-member order, members keeping the leaf-first order they arrived in. A flat
// graph yields one group per entity.
// Mirrors docs/consumers/gen.md#grouping-by-component.
pub fn groups_in_order(store: &Store, ordered: &[String]) -> Vec<(String, Vec<String>)> {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for id in ordered {
        let root = group_root(store, id);
        match groups.iter_mut().find(|(r, _)| *r == root) {
            Some((_, members)) => members.push(id.clone()),
            None => groups.push((root, vec![id.clone()])),
        }
    }
    groups
}

// A group is its root plus every descendant through `parent`, in tree order.
pub fn group_members(store: &Store, root: &str) -> Vec<String> {
    let mut members = vec![root.to_string()];
    let mut i = 0;
    while i < members.len() {
        let cur = members[i].clone();
        for (cid, e) in &store.graph.entities {
            if e.parent.as_deref() == Some(cur.as_str()) && !members.iter().any(|m| m == cid) {
                members.push(cid.clone());
            }
        }
        i += 1;
    }
    members
}

// Leaf-first order over the per-direction contributions and the containment tree:
// every contribution's acted-on side first (the part, the dependency, the
// interface), and a child before its parent (the part before the whole). Ties by
// id; a cycle breaks at the entity with the fewest pending prerequisites.
// Mirrors docs/consumers/gen.md#order-from-relationships.
pub fn generation_order(store: &Store, targets: &[String]) -> Vec<String> {
    let set: std::collections::BTreeSet<&str> = targets.iter().map(|s| s.as_str()).collect();
    let prereqs = |id: &str, emitted: &std::collections::BTreeSet<String>| -> usize {
        let mut n = 0;
        for rel in store.graph.relationships.values() {
            for c in &rel.contributions {
                if c.a == id && c.b != id && set.contains(c.b.as_str()) && !emitted.contains(&c.b) {
                    n += 1;
                }
            }
        }
        for (cid, e) in &store.graph.entities {
            if e.parent.as_deref() == Some(id)
                && set.contains(cid.as_str())
                && !emitted.contains(cid)
            {
                n += 1;
            }
        }
        n
    };
    let mut remaining: Vec<String> = targets.to_vec();
    let mut emitted: std::collections::BTreeSet<String> = Default::default();
    let mut out = Vec::new();
    while !remaining.is_empty() {
        let (i, _) = remaining
            .iter()
            .enumerate()
            .min_by_key(|(_, id)| (prereqs(id, &emitted), (*id).clone()))
            .unwrap();
        let id = remaining.remove(i);
        emitted.insert(id.clone());
        out.push(id);
    }
    out
}

// ---- ledger-stale change records ----
// One record per requirement or entity the ledger and the graph disagree about,
// derived from the same pending predicates the goals read, with the resolving goal
// and the reason in `detail`, so the board derives the bind, generate, and verify
// goals from them. Mirrors docs/compiler/goals/bind.md#created-when,
// docs/compiler/goals/generate.md#created-when, docs/compiler/goals/verify.md#created-when.
pub fn ledger_stale_records(store: &Store, gs: &GenSettings) -> Vec<crate::model::ChangeRecord> {
    let generation = store.status.generation;
    let mut index = store
        .status
        .changes
        .iter()
        .filter(|c| c.generation == generation)
        .map(|c| c.mutation)
        .max()
        .unwrap_or(0);
    let mut out: Vec<crate::model::ChangeRecord> = Vec::new();
    let mut push = |index: &mut usize, subject: String, detail: Value| {
        *index += 1;
        out.push(crate::model::ChangeRecord {
            id: format!("c{}-{}", generation, index),
            generation,
            mutation: *index,
            kind: crate::goals::CHANGE_LEDGER_STALE.to_string(),
            subject,
            via: "ledger".into(),
            detail,
        });
    };
    let bind_owed: std::collections::BTreeSet<String> = crate::bind::pending(store, gs)
        .iter()
        .filter_map(|p| p["requirement"].as_str().map(String::from))
        .collect();
    for p in crate::bind::pending(store, gs) {
        push(
            &mut index,
            p["requirement"].as_str().unwrap_or_default().to_string(),
            json!({"goal": "bind", "reason": p["reason"], "entity": p["entity"]}),
        );
    }
    for p in pending(store, gs) {
        let reason = match p["reason"].as_str() {
            Some("unimplemented-bindings") => json!("unimplemented"),
            Some("files-gone") => json!("files-gone"),
            _ => json!("facts-changed"),
        };
        push(
            &mut index,
            p["entity"].as_str().unwrap_or_default().to_string(),
            json!({"goal": "generate", "reason": reason, "changed": p["changed"]}),
        );
    }
    for p in crate::verify::pending(store, gs, Some("stale"), None) {
        if p["status"] == "unimplemented" || p["reason"] == "not-generated" {
            continue;
        }
        if bind_owed.contains(p["requirement"].as_str().unwrap_or_default()) {
            continue;
        }
        push(
            &mut index,
            p["requirement"].as_str().unwrap_or_default().to_string(),
            json!({"goal": "verify", "reason": p["reason"], "kind": p["test"]["kind"], "entity": p["entity"]}),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn fixture(out: &std::path::Path) -> (Store, GenSettings) {
        let mut s = Store {
            out: out.to_path_buf(),
            ..Default::default()
        };
        s.graph.entities.insert(
            "ent:cart".into(),
            Entity {
                name: "Cart".into(),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:shop-1".into(),
            Requirement {
                statement: "The Cart shall hold items.".into(),
                entities: vec!["ent:cart".into()],
                edges: vec![],
                source: Some(SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop".into(),
                    quote: "holds".into(),
                }),
                ..Default::default()
            },
        );
        let gs = GenSettings {
            deliverable: out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
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

    // A row whose requirement is a subject of an open error diagnostic: the package
    // names the diagnostic, the record flags the row, and the reply says so.
    // Mirrors docs/consumers/gen.md#rows-recorded-over-an-open-contradiction.
    #[test]
    fn a_row_over_an_open_error_diagnostic_is_flagged_contradicted() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-contra-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (mut s, gs) = fixture(&out);
        let diag = |severity: &str, lifecycle: &str| Diagnostic {
            rule: "contradiction".into(),
            severity: severity.into(),
            subjects: vec!["req:shop-1".into(), "req:shop-9".into()],
            message: "21 days versus 30 days".into(),
            reasoning: None,
            lifecycle: lifecycle.into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        };
        s.graph
            .diagnostics
            .insert("diag:contradiction-1".into(), diag("error", "open"));
        // A resolved error and an open warning on the same subject count for nothing.
        s.graph
            .diagnostics
            .insert("diag:contradiction-2".into(), diag("error", "resolved"));
        s.graph
            .diagnostics
            .insert("diag:contradiction-3".into(), diag("warning", "open"));
        let pkg = task_package(&s, "ent:cart", &gs).unwrap();
        let open = &pkg["requirementGroups"][0][0]["openDiagnostics"];
        assert_eq!(open.as_array().map(|a| a.len()), Some(1), "{}", pkg);
        assert_eq!(open[0]["id"], "diag:contradiction-1");
        assert_eq!(open[0]["rule"], "contradiction");
        assert_eq!(pkg["contradicted"], json!(["req:shop-1"]));

        std::fs::create_dir_all(gs.deliverable.join("tests")).ok();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        std::fs::write(gs.deliverable.join("cart.rs"), "fn hold() {}\n").ok();
        std::fs::write(
            gs.deliverable.join("tests/cart.rs"),
            format!("fn {}() {{}}\n", name),
        )
        .ok();
        let manifest = json!({
            "files": ["cart.rs", "tests/cart.rs"],
            "tests": [{
                "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
                "artifact": "tests/cart.rs", "name": name,
                "run": format!("cargo test {}", name), "files": ["cart.rs"],
            }],
        });
        let reply = mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
        assert_eq!(
            reply["contradicted"]["req:shop-1"],
            json!(["diag:contradiction-1"]),
            "{}",
            reply
        );
        assert_eq!(
            Ledger::load(&out).contradicted["req:shop-1"],
            vec!["diag:contradiction-1".to_string()]
        );
        // The dispute resolves; the next record clears the flag.
        s.graph
            .diagnostics
            .get_mut("diag:contradiction-1")
            .unwrap()
            .lifecycle = "resolved".into();
        let reply = mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
        assert!(reply.get("contradicted").is_none(), "{}", reply);
        assert!(Ledger::load(&out).contradicted.is_empty());
    }

    // The divergence check fires only between two known tool families.
    // Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
    #[test]
    fn medium_divergence_names_disjoint_tool_families() {
        let row = |run: &str| ReqRow {
            entity: "ent:cart".into(),
            files: Vec::new(),
            sites: Vec::new(),
            test: TestRef {
                kind: "programmatic".into(),
                label: "unit".into(),
                artifact: "tests/cart.rs".into(),
                name: "req_shop_1".into(),
                run: run.into(),
                cwd: ".".into(),
            },
            hashes: RowHashes::default(),
            verdict: "none".into(),
            last_run: None,
            exit_code: None,
            evidence: None,
        };
        let mut ledger = Ledger::default();
        ledger.medium = Some(Medium {
            form: "Python package".into(),
            produced: "written".into(),
            toolchain: "python3 with pytest".into(),
            artifact: String::new(),
        });
        assert!(medium_divergence(&ledger).is_none(), "no rows, no verdict");
        ledger
            .requirements
            .insert("req:shop-1".into(), row("cargo test req_shop_1"));
        let w = medium_divergence(&ledger).expect("python beside cargo diverges");
        assert!(
            w.contains("python") && w.contains("rust") && w.contains("cargo test"),
            "{}",
            w
        );
        // The same family on both sides, or an unknown one on either, is no divergence.
        ledger.medium.as_mut().unwrap().toolchain = "rustc and cargo test".into();
        assert!(medium_divergence(&ledger).is_none());
        ledger.medium.as_mut().unwrap().toolchain = "a bespoke DSL".into();
        assert!(medium_divergence(&ledger).is_none());
        ledger.medium.as_mut().unwrap().toolchain = "python3 with pytest".into();
        ledger
            .requirements
            .insert("req:shop-1".into(), row("sh tests/shop.sh"));
        assert!(medium_divergence(&ledger).is_none());
    }

    // Once an entity is generated the medium stands: the record warns and keeps it.
    // Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
    #[test]
    fn mark_warns_when_run_commands_diverge_from_the_medium() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-diverge-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        std::fs::create_dir_all(&out).ok();
        let mut ledger = Ledger::default();
        ledger.medium = Some(Medium {
            form: "Python package".into(),
            produced: "written".into(),
            toolchain: "python3 with pytest".into(),
            artifact: String::new(),
        });
        ledger.save(&out);
        std::fs::create_dir_all(gs.deliverable.join("tests")).ok();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        std::fs::write(gs.deliverable.join("cart.rs"), "fn hold() {}\n").ok();
        std::fs::write(
            gs.deliverable.join("tests/cart.rs"),
            format!("fn {}() {{}}\n", name),
        )
        .ok();
        let manifest = json!({
            "files": ["cart.rs", "tests/cart.rs"],
            "tests": [{
                "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
                "artifact": "tests/cart.rs", "name": name,
                "run": format!("cargo test {}", name), "files": ["cart.rs"],
            }],
        });
        let reply = mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
        let w = reply["mediumWarning"].as_str().expect("the record warns");
        assert!(w.contains("python") && w.contains("rust"), "{}", w);
        let ledger = Ledger::load(&out);
        assert_eq!(
            ledger.medium.as_ref().map(|m| m.toolchain.as_str()),
            Some("python3 with pytest"),
            "an entity is generated, so the medium stands"
        );
        // Every later package carries the standing divergence.
        let pkg = task_package(&s, "ent:cart", &gs).unwrap();
        assert!(pkg["mediumWarning"].as_str().is_some(), "{}", pkg);
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
        assert_eq!(
            task_package(&s, "ent:cart", &gs).unwrap()["build"]["run"],
            "python build_deck.py"
        );
        mark(
            &s,
            "ent:cart",
            None,
            &serde_json::json!({"files": ["src/cart.rs"], "build": {"run": "make all"}, "tests": []}),
            &gs,
        )
        .unwrap();
        assert_eq!(
            Ledger::load(&out).build.unwrap().run,
            "python build_deck.py"
        );
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

        let two =
            parse_file_replies("FILE: src/a.py\nprint(1)\n\nFILE: build.py\nimport a\na.go()\n")
                .unwrap();
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

    // An entity with no ledger entry is generation work through an unimplemented
    // bound row only: binding classifies first, and adopted code whose rows read
    // verified is never generated over. Mirrors docs/compiler/goals/generate.md#created-when.
    #[test]
    fn pending_diff_and_mark_lifecycle() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-test-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        assert!(pending(&s, &gs).is_empty(), "unbound: bind first");
        assert!(skip_reason(&s, &gs, "ent:cart").contains("owe a bind"));

        // Binding finds nothing: the row reads unimplemented and the entity is work.
        std::fs::create_dir_all(gs.deliverable.join("src")).ok();
        std::fs::create_dir_all(gs.deliverable.join("tests")).ok();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        std::fs::write(
            gs.deliverable.join("tests/cart.rs"),
            format!("// req:shop-1\nfn {}() {{}}", name),
        )
        .ok();
        let bound = json!({"kind": "programmatic", "artifact": "tests/cart.rs", "name": name, "run": format!("cargo test {}", name)});
        crate::bind::record(&s, "req:shop-1", &[], &bound, "fail", None, &gs).unwrap();
        let p = pending(&s, &gs);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["reason"], "unimplemented-bindings");
        assert_eq!(p[0]["changed"][0], "req:shop-1");

        // Binding that found the code verified: adopted, nothing to generate.
        std::fs::write(gs.deliverable.join("src/cart.rs"), "// product").ok();
        crate::bind::record(&s, "req:shop-1", &["src/cart.rs".into()], &bound, "pass", None, &gs)
            .unwrap();
        assert!(pending(&s, &gs).is_empty(), "adopted code is not generation work");
        assert!(skip_reason(&s, &gs, "ent:cart").contains("no ledger entry"));
        crate::bind::record(&s, "req:shop-1", &[], &bound, "fail", None, &gs).unwrap();

        // A mark with a manifest whose files exist makes it disappear from pending and
        // seeds a verification row.
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
        assert_eq!(skip_reason(&s, &gs, "ent:cart"), "unchanged");
        let ledger = Ledger::load(&out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(row.verdict, "none");
        assert_eq!(
            row.hashes.requirement,
            hash_hex("The Cart shall hold items.")
        );

        // A recorded file deleted by hand is generation work under its own name.
        std::fs::remove_file(gs.deliverable.join("src/cart.rs")).unwrap();
        let p = pending(&s, &gs);
        assert_eq!(p[0]["reason"], "files-gone");
        assert_eq!(p[0]["changed"][0], "src/cart.rs");
        let rec = ledger_stale_records(&s, &gs);
        let g = rec.iter().find(|c| c.detail["goal"] == "generate").unwrap();
        assert_eq!(g.detail["reason"], "files-gone");
        std::fs::write(gs.deliverable.join("src/cart.rs"), "// product").ok();

        // A new requirement reappears as a precise diff.
        let mut s2 = s.clone();
        s2.graph.requirements.insert(
            "req:shop-2".into(),
            Requirement {
                statement: "The Cart shall empty on checkout.".into(),
                entities: vec!["ent:cart".into()],
                edges: vec![],
                source: Some(SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop".into(),
                    quote: "empty".into(),
                }),
                ..Default::default()
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
        assert!(g0[0]["testName"]
            .as_str()
            .unwrap()
            .starts_with("req_shop_1_"));
    }

    // The example-sort livelock: rows for deleted requirements had no legal removal.
    // Any record buries them. Mirrors docs/consumers/gen.md#deletion-prunes-the-ledger.
    #[test]
    fn mark_prunes_rows_whose_requirement_left_the_graph() {
        let out = std::env::temp_dir().join(format!("jazyk-prune-test-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        std::fs::create_dir_all(gs.deliverable.join("tests")).ok();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        std::fs::write(
            gs.deliverable.join("tests/cart.rs"),
            format!("// req:shop-1\nfn {}() {{}}", name),
        )
        .ok();
        // A leftover row for a requirement the graph no longer holds.
        let mut ledger = Ledger::load(&out);
        let gone: ReqRow = serde_json::from_value(serde_json::json!({
            "entity": "ent:cart",
            "test": {"kind": "programmatic", "label": "unit", "artifact": "tests/gone.rs",
                     "name": "req_gone_1_x", "run": "cargo test req_gone_1_x"},
            "hashes": {"requirement": "x", "test": "x", "files": "x"},
            "verdict": "fail",
        }))
        .unwrap();
        ledger.requirements.insert("req:gone-1".into(), gone);
        ledger.save(&out);
        let manifest = serde_json::json!({
            "files": ["tests/cart.rs"],
            "tests": [{
                "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
                "artifact": "tests/cart.rs", "name": name, "run": format!("cargo test {}", name),
            }],
        });
        let r = mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
        assert_eq!(r["prunedRows"], 1);
        let ledger = Ledger::load(&out);
        assert!(!ledger.requirements.contains_key("req:gone-1"));
        assert!(ledger.requirements.contains_key("req:shop-1"));
    }

    // Every path a manifest names is confined before any side effect: an absolute
    // path or one climbing out with `..` is rejected naming it, and the rejected
    // record strips no marker, writes no ledger, and removes nothing.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    #[test]
    fn manifest_paths_are_confined_to_the_deliverable() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-confine-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        std::fs::create_dir_all(gs.deliverable.join("src")).ok();
        std::fs::create_dir_all(gs.deliverable.join("tests")).ok();
        // A file beside the deliverable that a climbing path could reach.
        std::fs::write(out.join("secret.txt"), "// req:shop-1 hash:deadbeef\nkeep\n").unwrap();
        std::fs::write(gs.deliverable.join("src/cart.rs"), "// req:shop-1 hash:deadbeef\nfn a() {}\n").unwrap();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        std::fs::write(gs.deliverable.join("tests/cart.rs"), format!("fn {}() {{}}\n", name)).unwrap();
        let row = json!({
            "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
            "artifact": "tests/cart.rs", "name": name, "run": format!("cargo test {}", name),
        });
        for (bad, field) in [
            (json!({"files": ["src/cart.rs", "../secret.txt"], "tests": [row.clone()]}), "files"),
            (json!({"files": ["/etc/passwd"], "tests": [row.clone()]}), "files"),
            (json!({"files": ["src/cart.rs"], "supportFiles": ["../../x"], "tests": [row.clone()]}), "supportFiles"),
            (json!({"files": ["src/cart.rs"], "tests": [{"requirement": "req:shop-1", "kind": "programmatic", "artifact": "../secret.txt", "name": name, "run": "x"}]}), "artifact"),
            (json!({"files": ["src/cart.rs"], "tests": [row.clone()], "build": {"run": "make", "cwd": "..", "produces": []}}), "build.cwd"),
        ] {
            let err = mark(&s, "ent:cart", None, &bad, &gs).unwrap_err();
            assert!(err.contains(field), "{}: {}", field, err);
            assert!(!Ledger::path(&out).exists(), "a rejected record writes no ledger");
        }
        // No side effect reached either file: the markers are still there.
        assert!(std::fs::read_to_string(out.join("secret.txt")).unwrap().contains("hash:"));
        assert!(std::fs::read_to_string(gs.deliverable.join("src/cart.rs")).unwrap().contains("hash:"));
        // The clean spellings are normalized on record.
        let ok = json!({"files": ["./src/cart.rs", "src//cart.rs"], "tests": [row]});
        mark(&s, "ent:cart", None, &ok, &gs).unwrap();
        assert_eq!(Ledger::load(&out).entities["cart"].files, vec!["src/cart.rs".to_string()]);
        assert_eq!(confine_rel("a\\b\\c.py").unwrap(), "a/b/c.py");
        assert!(confine_rel("C:/x").is_err());
        assert!(confine_rel(".").is_err());
        assert_eq!(confine_cwd("").unwrap(), ".");
        std::fs::remove_dir_all(&out).ok();
    }

    // The record applies the artifact gate binding applies: a row whose artifact is
    // not there, or a programmatic row whose artifact lacks the declared name, is
    // rejected naming the row and the fix. Mirrors docs/compiler/goals/generate.md#gate.
    #[test]
    fn record_generation_rejects_a_test_its_artifact_does_not_carry() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-artgate-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        std::fs::create_dir_all(gs.deliverable.join("tests")).ok();
        std::fs::write(gs.deliverable.join("cart.rs"), "fn a() {}\n").unwrap();
        std::fs::write(gs.deliverable.join("tests/cart.rs"), "fn something_else() {}\n").unwrap();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        let absent = json!({"files": ["cart.rs"], "tests": [{
            "requirement": "req:shop-1", "kind": "programmatic", "artifact": "tests/cart.rs",
            "name": name, "run": format!("cargo test {}", name)}]});
        let err = mark(&s, "ent:cart", None, &absent, &gs).unwrap_err();
        assert!(err.contains("does not contain that name") && err.contains(&name), "{}", err);
        let gone = json!({"files": ["cart.rs"], "tests": [{
            "requirement": "req:shop-1", "kind": "programmatic", "artifact": "tests/nope.rs",
            "name": name, "run": format!("cargo test {}", name)}]});
        let err = mark(&s, "ent:cart", None, &gone, &gs).unwrap_err();
        assert!(err.contains("no such file"), "{}", err);
        let no_criteria = json!({"files": ["cart.rs"], "tests": [{
            "requirement": "req:shop-1", "kind": "llm", "artifact": "criteria/req-shop-1.md", "name": name}]});
        let err = mark(&s, "ent:cart", None, &no_criteria, &gs).unwrap_err();
        assert!(err.contains("gen/"), "{}", err);
        assert!(mark(&s, "ent:cart", None, &json!({"files": ["cart.rs"], "tests": [{
            "requirement": "req:shop-1", "kind": "oracle", "artifact": "x", "name": "y", "run": "z"}]}), &gs)
            .unwrap_err()
            .contains("`programmatic`"));
        // The criteria file under the out directory satisfies an llm row.
        std::fs::create_dir_all(out.join("gen/criteria")).unwrap();
        std::fs::write(out.join("gen/criteria/req-shop-1.md"), "---\nrequirement: req:shop-1\n---\n").unwrap();
        mark(&s, "ent:cart", None, &no_criteria, &gs).unwrap();
        std::fs::remove_dir_all(&out).ok();
    }

    // A regeneration replaces the file set through the record itself, whichever
    // worker records: what the previous record listed and this manifest omits is
    // removed (snapshotted first), while a file another entity records, a support
    // file, or a row's test artifact stays.
    // Mirrors docs/consumers/gen.md#incremental-regeneration.
    #[test]
    fn a_record_removes_the_files_its_manifest_omits() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-remove-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (mut s, gs) = fixture(&out);
        s.graph.entities.insert("ent:shelf".into(), Entity { name: "Shelf".into(), ..Default::default() });
        std::fs::create_dir_all(gs.deliverable.join("src")).ok();
        std::fs::create_dir_all(gs.deliverable.join("tests")).ok();
        for f in ["src/cart.rs", "src/cart_old.rs", "src/shared.rs", "Cargo.toml", "src/cart_v2.rs"] {
            std::fs::write(gs.deliverable.join(f), format!("// {}\n", f)).unwrap();
        }
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        std::fs::write(gs.deliverable.join("tests/cart.rs"), format!("fn {}() {{}}\n", name)).unwrap();
        let row = json!({
            "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
            "artifact": "tests/cart.rs", "name": name, "run": format!("cargo test {}", name),
        });
        mark(&s, "ent:shelf", None, &json!({"files": ["src/shared.rs"], "tests": []}), &gs).unwrap();
        mark(&s, "ent:cart", None, &json!({
            "files": ["src/cart.rs", "src/cart_old.rs", "src/shared.rs", "tests/cart.rs"],
            "supportFiles": ["Cargo.toml"], "tests": [row.clone()],
        }), &gs).unwrap();
        // The second record drops cart_old, shared (the other entity's too), and the
        // support file from its list.
        let r = mark(&s, "ent:cart", None, &json!({
            "files": ["src/cart_v2.rs", "tests/cart.rs"], "tests": [row],
        }), &gs).unwrap();
        assert_eq!(r["removed"], json!(["src/cart.rs", "src/cart_old.rs"]), "{}", r);
        assert!(!gs.deliverable.join("src/cart_old.rs").exists());
        assert!(!gs.deliverable.join("src/cart.rs").exists());
        assert!(gs.deliverable.join("src/shared.rs").exists(), "another entity records it");
        assert!(gs.deliverable.join("Cargo.toml").exists(), "support files are the deliverable's");
        assert!(gs.deliverable.join("tests/cart.rs").exists());
        assert!(out.join("deliverable-baseline/src/cart_old.rs").exists(), "snapshotted before removal");
        std::fs::remove_dir_all(&out).ok();
    }

    // Part before whole: a parent's children generate first, and a system's direct
    // child roots its subtree as one group.
    // Mirrors docs/consumers/gen.md#grouping-by-component.
    #[test]
    fn children_generate_before_their_parent_and_groups_derive_from_containment() {
        let mut s = Store::default();
        for (id, parent) in [
            ("ent:sys", None),
            ("ent:svc", Some("ent:sys")),
            ("ent:svc-part", Some("ent:svc")),
            ("ent:other", Some("ent:sys")),
        ] {
            s.graph.entities.insert(
                id.into(),
                Entity {
                    name: id.into(),
                    parent: parent.map(String::from),
                    ..Default::default()
                },
            );
        }
        // The system is the containment root; its direct children root the groups.
        assert!(is_system(&s, "ent:sys"));
        assert!(!is_system(&s, "ent:svc"));
        assert_eq!(group_root(&s, "ent:svc-part"), "ent:svc");
        assert_eq!(group_root(&s, "ent:svc"), "ent:svc");
        assert_eq!(group_root(&s, "ent:sys"), "ent:sys");
        assert_eq!(
            group_members(&s, "ent:svc"),
            vec!["ent:svc".to_string(), "ent:svc-part".to_string()]
        );
        // Order: every child before its parent, the system last.
        let order = generation_order(
            &s,
            &[
                "ent:sys".into(),
                "ent:svc".into(),
                "ent:svc-part".into(),
                "ent:other".into(),
            ],
        );
        let pos = |id: &str| order.iter().position(|x| x == id).unwrap();
        assert!(pos("ent:svc-part") < pos("ent:svc"), "{:?}", order);
        assert!(pos("ent:svc") < pos("ent:sys"), "{:?}", order);
        assert!(pos("ent:other") < pos("ent:sys"), "{:?}", order);
        // run_all batches by these groups: svc-part rides with its root svc, the
        // system and the childless direct child are groups of their own.
        let groups = groups_in_order(&s, &order);
        let of = |root: &str| {
            groups
                .iter()
                .find(|(r, _)| r == root)
                .map(|(_, m)| m.clone())
                .unwrap_or_default()
        };
        assert!(of("ent:svc").contains(&"ent:svc".to_string()));
        assert!(of("ent:svc").contains(&"ent:svc-part".to_string()));
        assert_eq!(of("ent:sys"), vec!["ent:sys".to_string()]);
        assert_eq!(of("ent:other"), vec!["ent:other".to_string()]);
    }

    // The fact hash covers everything the ledger documents: stereotype, attributes,
    // and a statement's edges flip it, so those changes become generation work.
    // Mirrors docs/consumers/gen.md#the-ledger.
    #[test]
    fn fact_hash_flips_on_stereotype_attributes_and_edges() {
        let out = std::env::temp_dir().join(format!("jazyk-facthash-test-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (mut s, _gs) = fixture(&out);
        let h0 = fact_hash(&s, "ent:cart");
        s.graph.entities.get_mut("ent:cart").unwrap().stereotype = Some("service".into());
        let h1 = fact_hash(&s, "ent:cart");
        assert_ne!(h0, h1, "a stereotype change flips the hash");
        s.graph
            .entities
            .get_mut("ent:cart")
            .unwrap()
            .attributes
            .push(Attribute {
                name: "capacity".into(),
                r#type: Some("u32".into()),
                value: None,
                provenance: Provenance::Quote(SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop".into(),
                    quote: "holds".into(),
                }),
            });
        let h2 = fact_hash(&s, "ent:cart");
        assert_ne!(h1, h2, "an attribute change flips the hash");
        s.graph
            .requirements
            .get_mut("req:shop-1")
            .unwrap()
            .edges
            .push(ReqEdge {
                a: "ent:cart".into(),
                b: "ent:item".into(),
                rel_type: Some("composition".into()),
                cardinality: Some("1..*".into()),
            });
        let h3 = fact_hash(&s, "ent:cart");
        assert_ne!(h2, h3, "an edge change flips the hash");
        std::fs::remove_dir_all(&out).ok();
    }

    // One invented-choice diagnostic per manifest entry, graded by the scope of the
    // invention, with the proposal prompt and the measure in the message.
    // Mirrors docs/consumers/gen.md#invented-choices.
    #[test]
    fn invented_choices_stage_one_graded_diagnostic_per_scope() {
        let out = std::env::temp_dir().join(format!("jazyk-choice-test-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (mut s, gs) = fixture(&out);
        s.docs.insert(
            "shop.md".into(),
            DocRecord {
                content_hash: hash_hex("# Shop\n\nholds\n"),
                sections: crate::md::parse_sections("# Shop\n\nholds\n"),
                coverage: Default::default(),
            },
        );
        s.graph.requirements.insert(
            "req:shop-2".into(),
            Requirement {
                statement: "The Cart shall print items.".into(),
                entities: vec!["ent:cart".into()],
                edges: vec![],
                source: Some(SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop".into(),
                    quote: "holds".into(),
                }),
                ..Default::default()
            },
        );
        std::fs::create_dir_all(gs.deliverable.join("src")).ok();
        std::fs::write(gs.deliverable.join("src/cart.rs"), "fn hold() {}\n").ok();
        // Distinct subject sets: the store's sticky rule merges open diagnostics
        // with one rule and one subject set, so the requirements a choice fills in
        // are what keep its diagnostic its own.
        let manifest = serde_json::json!({
            "files": ["src/cart.rs"],
            "tests": [],
            "choices": [
                {"choice": "The product is a command-line cart simulator.", "scope": "product",
                 "reasoning": "no statement names the medium"},
                {"choice": "The cart rejects a 21st item.", "scope": "behavior",
                 "reasoning": "no stated limit", "requirements": ["req:shop-1"]},
                {"choice": "Items print in green.", "scope": "detail", "reasoning": "cosmetic",
                 "requirements": ["req:shop-2"]},
            ],
        });
        let reply = mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
        assert_eq!(reply["choices"], 3);
        let choices = parse_choices(&s, &manifest).unwrap();
        let ledger = Ledger::load(&out);
        let ops = choice_ops(
            &s,
            "ent:cart",
            &choices,
            ledger.entities["cart"].unattached.as_ref(),
        );
        assert_eq!(ops.len(), 3);
        let report = s.apply(ops, &crate::store::Commit::store("session"));
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        let staged: Vec<&Diagnostic> = s
            .graph
            .diagnostics
            .values()
            .filter(|d| d.rule == "invented-choice")
            .collect();
        assert_eq!(staged.len(), 3);
        for want in ["error", "warning", "info"] {
            assert!(staged.iter().any(|d| d.severity == want), "{}", want);
        }
        let behavior = staged.iter().find(|d| d.severity == "warning").unwrap();
        assert!(behavior.subjects.contains(&"ent:cart".to_string()));
        assert!(behavior.subjects.contains(&"req:shop-1".to_string()));
        assert!(
            behavior.message.contains("Unattached remainder"),
            "{}",
            behavior.message
        );
        let p = behavior.prompt.as_ref().unwrap();
        let edit = p.options[0].edit.as_ref().unwrap();
        assert_eq!(edit.doc, "shop.md");
        assert_eq!(edit.section, "/shop");
        assert_eq!(edit.old_text, "");
        assert_eq!(edit.new_text, "The cart rejects a 21st item.");
        assert_eq!(p.options[1].answer.as_deref(), Some("keep unstated"));
        assert!(p.freeform);
        // A repeated choice keeps its diagnostic; an omitted one resolves.
        let again = parse_choices(
            &s,
            &serde_json::json!({"choices": [
                {"choice": "The cart rejects a 21st item.", "scope": "behavior", "reasoning": "still unstated"},
            ]}),
        )
        .unwrap();
        let ops = choice_ops(&s, "ent:cart", &again, None);
        let reports = ops
            .iter()
            .filter(|o| matches!(o, crate::store::Op::ReportDiagnostic { .. }))
            .count();
        let resolves = ops
            .iter()
            .filter(|o| matches!(o, crate::store::Op::ResolveDiagnostic { .. }))
            .count();
        assert_eq!(reports, 0);
        assert_eq!(resolves, 2);
        // A bad scope is rejected before any side effect.
        let bad = serde_json::json!({"files": [], "tests": [], "choices": [{"choice": "x", "scope": "huge"}]});
        assert!(mark(&s, "ent:cart", None, &bad, &gs)
            .unwrap_err()
            .contains("scope"));
        std::fs::remove_dir_all(&out).ok();
    }

    // The unattached remainder on a two-file fixture: one owned file no row names,
    // and the significant lines outside every site's run.
    // Mirrors docs/consumers/gen.md#the-unattached-remainder.
    #[test]
    fn unattached_remainder_counts_unnamed_files_and_lines_outside_site_runs() {
        let out = std::env::temp_dir().join(format!("jazyk-unattached-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        std::fs::create_dir_all(gs.deliverable.join("src")).ok();
        std::fs::create_dir_all(gs.deliverable.join("tests")).ok();
        // Two significant lines sit before the first site's run.
        std::fs::write(
            gs.deliverable.join("src/cart.rs"),
            "// header comment\nuse std::fmt;\nfn helper() {}\n// req:shop-1 hash:12345678\nfn hold(i: Item) {}\nfn hold_more() {}\n",
        )
        .ok();
        // A second owned file no requirement row names: unattached whole.
        std::fs::write(
            gs.deliverable.join("src/extra.rs"),
            "fn extra() {}\n\n// note\nfn more() {}\n",
        )
        .ok();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        std::fs::write(
            gs.deliverable.join("tests/cart.rs"),
            format!("fn {}() {{}}\n", name),
        )
        .ok();
        let manifest = serde_json::json!({
            "files": ["src/cart.rs", "src/extra.rs", "tests/cart.rs"],
            "tests": [{
                "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
                "artifact": "tests/cart.rs", "name": name,
                "run": format!("cargo test {}", name), "files": ["src/cart.rs"],
            }],
        });
        let reply = mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
        // src/extra.rs is the file slice; the lines are its two significant lines
        // plus src/cart.rs's two before the first site. The test artifact is claimed
        // by its row and never counts.
        assert_eq!(reply["unattached"]["files"], 1);
        assert_eq!(reply["unattached"]["lines"], 4);
        let ledger = Ledger::load(&out);
        let u = ledger.entities["cart"].unattached.as_ref().unwrap();
        assert_eq!(u.files, 1);
        assert_eq!(u.lines, 4);
        assert!(u.ratio > 0.5 && u.ratio < 1.0, "{}", u.ratio);
        std::fs::remove_dir_all(&out).ok();
    }

    // An unbound requirement yields a ledger-stale change record naming its goal,
    // so the board derives bind (and generate for the entity) from it.
    // Mirrors docs/compiler/goals/bind.md#created-when.
    #[test]
    fn an_unbound_requirement_yields_a_ledger_stale_record() {
        let out = std::env::temp_dir().join(format!("jazyk-stale-rec-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        let records = ledger_stale_records(&s, &gs);
        let bind = records
            .iter()
            .find(|r| r.detail["goal"] == "bind")
            .expect("a bind record");
        assert_eq!(bind.kind, "ledger-stale");
        assert_eq!(bind.subject, "req:shop-1");
        assert_eq!(bind.via, "ledger");
        assert_eq!(bind.detail["reason"], "unbound");
        assert!(bind.id.starts_with('c'), "{}", bind.id);
        // Binding classifies first: no generate record while the requirement is
        // unbound (docs/compiler/goals/generate.md#created-when).
        assert!(
            records.iter().all(|r| r.detail["goal"] != "generate"),
            "{:?}",
            records.iter().map(|r| r.detail.clone()).collect::<Vec<_>>()
        );
        // Bound and unimplemented: the entity is generation work under that reason.
        std::fs::create_dir_all(gs.deliverable.join("tests")).ok();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        std::fs::write(gs.deliverable.join("tests/cart.rs"), format!("fn {}() {{}}", name)).ok();
        let bound = json!({"kind": "programmatic", "artifact": "tests/cart.rs", "name": name, "run": format!("cargo test {}", name)});
        crate::bind::record(&s, "req:shop-1", &[], &bound, "fail", None, &gs).unwrap();
        let records = ledger_stale_records(&s, &gs);
        let g = records
            .iter()
            .find(|r| r.detail["goal"] == "generate")
            .expect("a generate record");
        assert_eq!(g.subject, "ent:cart");
        assert_eq!(g.detail["reason"], "unimplemented");
        assert_eq!(g.detail["changed"][0], "req:shop-1");
        std::fs::remove_dir_all(&out).ok();
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
    runner: &crate::acp::runner::AcpRunner,
    gs: &GenSettings,
    entities: &[String],
    force: bool,
    trace: &crate::session::Trace,
) -> Result<Value, String> {
    use crate::session::TraceEvent;
    let targets: Vec<String> = if entities.is_empty() {
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
            let medium = decide_medium(store, runner, &gs.deliverable)?;
            trace.line("gen medium", &medium.line());
            ledger.medium = Some(medium);
            ledger.save(&store.out);
        }
    }

    // Leaf-first over the per-direction contributions and the containment tree: the
    // part before the whole, the dependency before the dependent, so a group's parts
    // land before the entity that composes them
    // (docs/consumers/gen.md#grouping-by-component).
    let ordered = generation_order(store, &targets);

    std::fs::create_dir_all(&gs.deliverable).ok();
    let pending_set: std::collections::BTreeSet<String> = pending(store, gs)
        .iter()
        .filter_map(|p| p["entity"].as_str().map(String::from))
        .collect();
    let (mut regenerated, mut skipped, mut failures) = (0u64, 0u64, 0u64);
    // One session per component group: the ready goals of one group run together, so
    // the parts of one component are written under one set of conventions. A member
    // whose facts match the ledger is skipped; its files ride the packages as
    // context (docs/consumers/gen.md#grouping-by-component).
    for (root, members) in groups_in_order(store, &ordered) {
        if trace.is_cancelled() {
            break;
        }
        let mut ready: Vec<String> = Vec::new();
        for id in &members {
            if !force && !pending_set.contains(id) {
                skipped += 1;
                trace.event(TraceEvent::GenEntitySkipped {
                    entity: id.clone(),
                    reason: skip_reason(store, gs, id),
                });
            } else {
                ready.push(id.clone());
            }
        }
        if ready.is_empty() {
            continue;
        }
        for id in &ready {
            trace.event(TraceEvent::GenEntityStart { entity: id.clone() });
        }
        let results: Vec<(String, Result<usize, String>)> = if gs.worker == "pipeline" {
            // The pipeline worker is a fixed per-entity sequence; the group still
            // orders it, part before whole.
            let mut out = Vec::new();
            for id in &ready {
                if trace.is_cancelled() {
                    break;
                }
                match task_package(store, id, gs) {
                    Ok(task) => out.push((id.clone(), gen_one(store, runner, gs, id, &task))),
                    Err(e) => {
                        trace.event(TraceEvent::GenEntityFailed {
                            entity: id.clone(),
                            stage: "task".into(),
                            error: e,
                        });
                        failures += 1;
                    }
                }
            }
            out
        } else {
            gen_session(store, runner, gs, &root, &ready, trace)
        };
        for (id, result) in results {
            match result {
                Ok(files) => {
                    trace.event(TraceEvent::GenEntityDone { entity: id, files });
                    regenerated += 1;
                }
                Err(e) => {
                    trace.event(TraceEvent::GenEntityFailed {
                        entity: id,
                        stage: "generate".into(),
                        error: e,
                    });
                    failures += 1;
                }
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
    Ok(
        json!({ "regenerated": regenerated, "skipped": skipped, "failures": failures, "build": build }),
    )
}

// Every file a reply carries, in order. A step asks for one, but a model that writes
// its module and the entry point in a single answer has written both, and folding the
// second into the first leaves a file that cannot parse.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
// The agentic worker: one component group as one generation session on the harness,
// with the file and command tools. The batch carries one generate goal per ready
// member, so the parts of one component are written together under one set of
// conventions (docs/consumers/gen.md#grouping-by-component). Success is the ledger's
// word, never the model's: the session must have left record_generation's mark with
// current facts and existing files for each member.
// Mirrors docs/compiler/sessions.md#generation-sessions.
fn gen_session(
    store: &Store,
    runner: &crate::acp::runner::AcpRunner,
    gs: &GenSettings,
    root: &str,
    ids: &[String],
    trace: &crate::session::Trace,
) -> Vec<(String, Result<usize, String>)> {
    let goals: Vec<crate::model::Goal> = ids
        .iter()
        .map(|id| crate::model::Goal {
            id: format!("g:generate:{}", id),
            kind: "generate".into(),
            class: "compile".into(),
            mandatory: true,
            target: id.to_string(),
            unit: "entity".into(),
            change: serde_json::json!({"goal": "generate"}),
            cause: None,
            state: crate::model::GoalState::Open,
            hints: Vec::new(),
        })
        .collect();
    // The group root labels the session; a group of one keeps its goal id.
    let batch = crate::acp::runner::BatchRun {
        id: format!("g:generate:{}", root),
        goals,
        executor: None,
    };
    let report = runner.run_item(&batch, trace);
    if let Some(e) = report.failed {
        return ids.iter().map(|id| (id.clone(), Err(e.clone()))).collect();
    }
    let ledger = Ledger::load(&store.out);
    ids.iter()
        .map(|id| {
            let hash = fact_hash(store, id);
            let res = match ledger.entities.get(&slug_of(id)) {
                Some(e)
                    if e.fact_hash == hash
                        && !e.files.is_empty()
                        && e.files.iter().all(|f| gs.deliverable.join(f).exists()) =>
                {
                    Ok(e.files.len())
                }
                Some(e) if e.fact_hash != hash => Err(format!(
                    "the session recorded factHash {} but the graph says {}; the goal package carries the current one",
                    e.fact_hash, hash
                )),
                Some(_) => Err(
                    "the session recorded files that do not exist under the deliverable".into(),
                ),
                None => Err(
                    "the session ended without record_generation; nothing landed in the ledger"
                        .into(),
                ),
            };
            (id.clone(), res)
        })
        .collect()
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
    let reply = match reply
        .lines()
        .position(|l| l.trim_start().starts_with("FILE:"))
    {
        Some(0) | None => reply.clone(),
        Some(n) => reply.lines().skip(n).collect::<Vec<_>>().join("\n"),
    };
    let mut lines = reply.splitn(2, '\n');
    let first = lines.next().unwrap_or("").trim();
    let Some(path) = first.strip_prefix("FILE:") else {
        return Err(format!(
            "first line must be `FILE: <path>`, got `{}`",
            crate::llm::truncate(first, 80)
        ));
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
            return body
                .split_once('\n')
                .map(|(_, b)| b)
                .unwrap_or(body)
                .to_string();
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
fn snapshot_baseline(
    out: &Path,
    gs: &GenSettings,
    rel: &str,
    seen: &mut std::collections::HashSet<String>,
) {
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

pub fn gen_one(
    store: &Store,
    runner: &crate::acp::runner::AcpRunner,
    gs: &GenSettings,
    id: &str,
    task: &serde_json::Value,
) -> Result<usize, String> {
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
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
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
    let groups = task["requirementGroups"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let parts = groups.len();
    let req_line = |r: &serde_json::Value| {
        format!(
            "- {} [{}]: {}\n  quote: {}",
            r["id"].as_str().unwrap_or(""),
            r["testName"].as_str().unwrap_or(""),
            r["statement"].as_str().unwrap_or(""),
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
        let req_lines: Vec<String> = group
            .as_array()
            .map(|a| a.iter().map(req_line).collect())
            .unwrap_or_default();
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
        let reply = runner
            .ask(
                instructions,
                &user,
                &format!("gen {}", id),
                &format!("product {}/{}", k + 1, parts),
            )
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
                    let again = runner
                        .ask(
                            instructions,
                            &retry,
                            &format!("gen {}", id),
                            "product format retry",
                        )
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
                let reply2 = runner
                    .ask(
                        instructions,
                        &retry,
                        &format!("gen {}", id),
                        "product retry",
                    )
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
    let all_reqs: Vec<serde_json::Value> = groups
        .iter()
        .flat_map(|g| g.as_array().cloned().unwrap_or_default())
        .collect();
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
    let tests_reply = runner
        .ask(instructions, &tests_user, &format!("gen {}", id), "tests")
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
                let again = runner
                    .ask(
                        instructions,
                        &retry,
                        &format!("gen {}", id),
                        "tests format retry",
                    )
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
                let reply2 = runner
                    .ask(instructions, &retry, &format!("gen {}", id), "tests retry")
                    .map_err(|e| format!("tests retry: {}", e))?;
                let (p, b) =
                    parse_file_reply(&reply2).map_err(|e| format!("tests reply: {}", e))?;
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
        "Files written so far for entity {}: {:?} under the deliverable directory `{}`.\nRecorded run commands already in use (reuse this toolchain, never introduce a second test runner): {}\nBuild already recorded for this deliverable: {}\nThe build's entry point and its current content (rewrite it in supportFiles so it includes your part; empty when there is no build yet): {}\nTest names the harness found in the tests file (declare each of these programmatic with its run command; declare a requirement whose name is absent as llm): {}\nEach programmatic run command must invoke the tests artifact and select only that test: the command text must reference the tests file path or the testName. A command that only runs the product is invalid.\nReturn ONLY a JSON object, no prose:\n{{\"supportFiles\": [{{\"path\": \"...\", \"content\": \"...\"}}], \"build\": {{\"run\": \"...\", \"cwd\": \".\", \"produces\": [\"...\"]}}, \"tests\": [{{\"requirement\": \"req:...\", \"kind\": \"programmatic\"|\"llm\", \"label\": \"your words\", \"name\": \"the testName\", \"run\": \"exact command executed from the deliverable directory that runs only that test\", \"cwd\": \".\"}}], \"choices\": [{{\"choice\": \"one sentence\", \"scope\": \"product\"|\"behavior\"|\"detail\", \"reasoning\": \"...\", \"requirements\": [\"req:...\"]}}]}}\nsupportFiles are build or configuration files required for the run commands to execute (empty array if none are needed or they already exist). choices lists what you had to invent because no statement decides it: the choice in one sentence, its scope (product, behavior, or detail), your reasoning, and the requirements it fills in when any exist; an empty array when you invented nothing. A run command that cannot execute from a fresh checkout of the deliverable is a defect: if it needs a runner or build file no listed file provides (a package.json for npx jest, a Cargo.toml for cargo test), you MUST return that file in supportFiles.\n{} Every requirement must appear once in tests. Requirements and test names:\n{}\n\nThe tests file `{}`:\n{}\n",
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
    let manifest_reply = runner
        .ask(
            instructions,
            &manifest_user,
            &format!("gen {}", id),
            "manifest",
        )
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
            let again = runner
                .ask(
                    instructions,
                    &retry,
                    &format!("gen {}", id),
                    "manifest format retry",
                )
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
                    Some(norm_rel(f["path"].as_str().unwrap_or_default()))
                        == entry_path.as_ref().map(|p| norm_rel(p))
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
                    let stem = std::path::Path::new(p)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string());
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
        let given = !manifest_json["build"]["run"]
            .as_str()
            .unwrap_or("")
            .trim()
            .is_empty();
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
        if let Ok(reply2) = runner.ask(
            instructions,
            &retry,
            &format!("gen {}", id),
            "manifest retry",
        ) {
            if let Ok(mut v) = parse_manifest(&reply2) {
                // Merge over the first answer: a retry that forgot a field keeps it.
                for key in ["supportFiles", "build", "choices"] {
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
            let (Some(path), Some(content)) = (f["path"].as_str(), f["content"].as_str()) else {
                continue;
            };
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
    let declared = manifest_json["tests"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut tests_manifest: Vec<serde_json::Value> = Vec::new();
    for r in &all_reqs {
        let rid = r["id"].as_str().unwrap_or_default();
        let name = r["testName"].as_str().unwrap_or_default();
        let row = declared
            .iter()
            .find(|t| t["requirement"].as_str() == Some(rid));
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
                r["statement"].as_str().unwrap_or_default(),
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
        .map(|a| {
            a.iter()
                .filter_map(|f| f["path"].as_str().map(norm_rel))
                .collect()
        })
        .unwrap_or_default();
    let build_entry_path = task["buildEntry"]["path"]
        .as_str()
        .map(norm_rel)
        .or_else(|| {
            entry_from_run(manifest_json["build"]["run"].as_str().unwrap_or(""))
                .map(|p| norm_rel(&p))
        });
    for p in extra_written {
        let n = norm_rel(&p);
        let deliverable_wide = declared_support.iter().any(|d| *d == n)
            || build_entry_path.as_deref() == Some(n.as_str());
        if deliverable_wide {
            support_files.push(p);
        } else if !files.contains(&p) {
            files.push(p);
        }
    }
    let mut manifest =
        serde_json::json!({"files": files, "tests": tests_manifest, "supportFiles": support_files});
    // The build the manifest step returned rides along: it is the deliverable's, not
    // this row's, and mark records it once (docs/consumers/gen.md#the-build).
    if !manifest_json["build"]["run"]
        .as_str()
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        manifest["build"] = manifest_json["build"].clone();
    }
    // The invented choices ride the manifest; mark validates them and the tool layer
    // stages their diagnostics (docs/consumers/gen.md#invented-choices).
    if manifest_json["choices"].is_array() {
        manifest["choices"] = manifest_json["choices"].clone();
    }
    // The record removes what the previous generation listed and this manifest omits
    // (docs/consumers/gen.md#incremental-regeneration).
    crate::gen::mark(store, id, task["factHash"].as_str(), &manifest, gs)?;
    // Both built-in workers file and clear the graded invented-choice diagnostics:
    // the session path stages them through record_generation, and this pipeline path
    // commits them here, right after the record they grade.
    // Mirrors docs/consumers/gen.md#invented-choices.
    let choices = parse_choices(store, &manifest)?;
    let unattached = Ledger::load(&store.out)
        .entities
        .get(&own_slug)
        .and_then(|e| e.unattached.clone());
    let mut ws = Store::load(&store.out);
    let ops = choice_ops(&ws, id, &choices, unattached.as_ref());
    if !ops.is_empty() {
        ws.apply(
            ops,
            &crate::store::Commit::session(vec![format!("g:generate:{}", id)], 0, 0),
        );
    }
    Ok(files.len())
}
