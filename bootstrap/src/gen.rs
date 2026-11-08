use crate::vfs::VPathExt;
pub(crate) mod progress;
mod pipeline;
pub(crate) mod assets;
#[cfg(test)]
mod matcher_recovery_tests;
#[cfg(test)]
mod asset_workflow_tests;
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
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct GenSettings {
    // Whether the project names a deliverable: the generation scope exists
    // (docs/compiler/goals/derive-criteria.md#created-when).
    pub configured: bool,
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
    pub(crate) fn current_for_store(&self, store: &Store) -> Self {
        let Some(root) = crate::project::find_root(&store.out) else { return self.clone() };
        let workspace = match crate::document_sets::Workspace::load(&root) {
            Ok(workspace) if workspace.exists => workspace,
            Ok(_) => return self.clone(),
            Err(_) => return Self { configured:false, code:vec![], ..self.clone() },
        };
        // Match the selected store, never the project-wide union of Source roles.
        workspace.sets.iter().filter_map(|set| workspace.resolve(Some(&set.id)).ok())
            .find(|project| project.out == store.out)
            .map(|project| Self::resolve(&project))
            .unwrap_or_else(|| Self { configured:false, code:vec![], ..self.clone() })
    }
    pub fn resolve(proj: &crate::project::Project) -> GenSettings {
        let deliverable = match &proj.gen_deliverable {
            Some(d) => proj.root.join(d),
            // Default: the project root itself. The docs glob keeps the source tree
            // out of the product's way (docs/compiler/project-settings.md#generation).
            None => proj.root.clone(),
        };
        GenSettings {
            configured: proj.gen_deliverable.is_some(),
            deliverable,
            worker: proj.gen_worker.clone().unwrap_or_else(|| "agentic".into()),
            code: proj.gen_code.clone(),
        }
    }

    // Placeholder for sessions with no project (benchmark cases). Gen tools are absent
    // from those toolsets, so the path is never read.
    pub fn allows_source(&self, relative: &str) -> bool {
        let Ok(w)=crate::document_sets::Workspace::load(&self.deliverable) else {return false;};
        if !w.exists {return true;}
        self.configured && crate::document_sets::safe_relative(&self.deliverable,relative)
            && crate::document_sets::included(&self.code,relative)
            && !w.sets.iter().any(|s|crate::document_sets::included(&s.docs,relative))
    }
    pub fn from_out(out: &Path) -> GenSettings {
        GenSettings {
            configured: false,
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
    // Set when gen/ledger.yaml existed but did not parse: the error. The ledger then
    // refuses to save, so a torn or hand-broken file is never replaced by an empty one.
    // Mirrors docs/consumers/gen.md#the-ledger.
    #[serde(skip)]
    pub unreadable: Option<String>,
    // Files that belong to the deliverable, not to an entity: command setup and
    // shared composition points, including a written artifact. Any task may extend
    // one while preserving its parts; exclusive entity ownership never applies.
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
    // Project tasks a ledger session added, keyed by task id. Only these are
    // swept once no ledger names them.
    // Mirrors docs/consumers/gen.md#session-tasks-are-swept-when-unused.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tasks: BTreeMap<String, TaskOwner>,
    #[serde(default)]
    pub entities: BTreeMap<String, EntityGen>,
    #[serde(default)]
    pub requirements: BTreeMap<String, ReqRow>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskOwner {
    pub created_by: String,
    pub created_at: String,
    // The task definition's hash when the session last saved it. A different
    // current definition is an author's edit: the task is the author's.
    pub definition: String,
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
// The note a package carries while no session has decided the medium: an external
// worker decides and records it with record_medium before writing anything.
// Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
pub fn medium_note(ledger: &Ledger) -> Option<String> {
    if ledger.medium.is_some() {
        return None;
    }
    Some(
        "MEDIUM UNDECIDED: no session has decided what this deliverable is made of. Decide it first from the requirements, the deliverable tree (an existing tree pins the language and toolchain), and any recorded run commands, and record it with record_medium before writing files; the contract's line that the medium is settled applies after that."
            .to_string(),
    )
}

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
pub fn run_build(out: &Path, gs: &GenSettings, trace: &crate::session::Trace, label: &str) -> Result<(),String> {
    run_build_receipt(out,gs,trace,label).map(|_|())
}

pub(crate) fn run_build_receipt(
    out: &Path,
    gs: &GenSettings,
    trace: &crate::session::Trace,
    label: &str,
) -> Result<Option<(String,String,Value)>, String> {
    let Some(b) = Ledger::load(out).build.clone() else {
        return Ok(None);
    };
    if let Some(id) = b.run.strip_prefix("task:") {
        let root = crate::task_runtime::project_root(out)?;
        let ledger = Ledger::load(out);
        let files: Vec<String> = ledger.entities.values().flat_map(|row| row.files.iter().cloned())
            .chain(ledger.support.iter().cloned()).collect();
        let inputs = hash_hex(&json!({"run":b.run,"cwd":b.cwd,"produces":b.produces,
            "sources":hash_files(gs,&files)}).to_string());
        let (run_id, result) = crate::task_runtime::execute_with_key(&root, id,
            &format!("build:{label}:{}:{inputs}", out.display()))?;
        let success = result["status"] == "succeeded" && result["complete"] == true;
        if !success {
            record_build_run(out, false, result["log"].as_str().unwrap_or("No log output"));
            return Err("Build task did not complete successfully. Inspect its run log.".into());
        }
        check_build_products(out, gs, &b)?;
        return Ok(Some((id.to_owned(),run_id,result)));
    }
    let cwd = gs.deliverable.join(&b.cwd);
    trace.line(label, &format!("build: {} (in {})", b.run, cwd.display()));
    let done = crate::sys::run_shell(&b.run, &cwd)
        .map_err(|e| {
            format!(
                "build `{}` could not start in {}: {}",
                b.run,
                cwd.display(),
                e
            )
        })?;
    if !done.success {
        let mut evidence = String::from_utf8_lossy(&done.stdout).to_string();
        evidence.push_str(&String::from_utf8_lossy(&done.stderr));
        // The next generation task needs to see this; printing it here only reaches
        // whoever is watching (docs/consumers/gen.md#the-build).
        record_build_run(out, false, &evidence);
        return Err(format!(
            "build `{}` failed ({}). Output:\n{}",
            b.run,
            done.status(),
            crate::llm::truncate(evidence.trim(), 2000)
        ));
    }
    check_build_products(out, gs, &b)?;
    Ok(None)
}

pub(crate) fn check_build_products(out: &Path, gs: &GenSettings, b: &Build) -> Result<(), String> {
    let missing: Vec<&String> = b
        .produces
        .iter()
        .filter(|p| !gs.deliverable.join(p).vexists())
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
    let _lock = Ledger::lock(out);
    let mut ledger = Ledger::load(out);
    if let Some(b) = ledger.build.as_mut() {
        b.last_run = Some(BuildRun {
            at: crate::verify::now_iso(),
            ok,
            error: crate::llm::truncate(error.trim(), 1500),
        });
        let _ = ledger.save(out);
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
    // Saved tasks this generation uses beyond its tests and the build.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<String>,
    // The unattached remainder, measured at record time: how much of what this
    // entity's generation produced no requirement claims.
    // Mirrors docs/consumers/gen.md#the-unattached-remainder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unattached: Option<Unattached>,
    // Display time only. Completion uses the sequence because timestamps have
    // one-second resolution. Mirrors docs/consumers/gen.md#the-ledger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_at: Option<String>,
    #[serde(default, skip_serializing_if = "record_sequence_is_zero")]
    pub record_sequence: u64,
}

fn record_sequence_is_zero(sequence: &u64) -> bool { *sequence == 0 }

// Generated mass attached to no requirement: owned files no row names, significant
// lines outside every site's run, and their share of the entity's significant lines.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Unattached {
    pub files: u64,
    pub lines: u64,
    pub ratio: f64,
    // No site anchors any owned line: the medium carries no markers and the ratio
    // says nothing. Mirrors docs/consumers/gen.md#the-unattached-remainder.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub markerless: bool,
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
    // One test per criterion, keyed by criterion id; `test` mirrors the first when
    // criteria exist. Mirrors docs/consumers/gen.md#the-ledger.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tests: BTreeMap<String, CritTest>,
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

// The test of one criterion: its reference, the criterion statement hash it encodes,
// the artifact hash, and the last verdict. Mirrors docs/consumers/gen.md#the-ledger.
#[derive(Serialize, Deserialize, Clone)]
pub struct CritTest {
    pub test: TestRef,
    pub criterion_hash: String,
    #[serde(default)]
    pub test_hash: String,
    #[serde(default = "verdict_none")]
    pub verdict: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
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

fn same_test_ref(a: &TestRef, b: &TestRef) -> bool {
    same_test_artifact(a, b)
        && a.run == b.run && a.cwd == b.cwd
}

fn same_test_artifact(a: &TestRef, b: &TestRef) -> bool {
    a.kind == b.kind && a.artifact == b.artifact && a.name == b.name
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
    strip_markers_in(text, None)
}

fn strip_markers_in(text: &str, set_id: Option<&str>) -> (String, Vec<RawSite>) {
    // `(?:req:)+` folds a doubled prefix (`req:req:orders-1`, a model pasting the
    // full id after a template's literal `req:`) into one clean site.
    let re = regex::Regex::new(r"(?:req:)+((?:ds_[A-Za-z0-9_-]+::)?[A-Za-z0-9][A-Za-z0-9_-]*)\s+hash:([0-9a-fA-F]{4,64})")
        .unwrap();
    let mut clean: Vec<&str> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    let mut sites: Vec<RawSite> = Vec::new();
    for line in text.lines() {
        if let Some(c) = re.captures(line) {
            let rest = line.replacen(c.get(0).unwrap().as_str(), "", 1);
            let rid = format!("req:{}", &c[1]);
            let (scope, local) = crate::webapi::sets::unqualify(&rid);
            let matches_set = set_id.is_none() || scope.is_none() || scope.as_deref() == set_id;
            if matches_set && !rest.chars().any(|ch| ch.is_alphanumeric()) {
                pending.push(if set_id.is_some() { local } else { rid });
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
    // The deliverable-wide file. The rows live one per file beside it.
    // Mirrors docs/consumers/gen.md#the-ledger.
    pub fn path(out: &Path) -> PathBuf {
        out.join("gen").join("ledger.yaml")
    }

    fn row_dir(out: &Path, kind: &str) -> PathBuf {
        out.join("gen").join(kind)
    }

    fn row_path(out: &Path, kind: &str, id: &str) -> PathBuf {
        Self::row_dir(out, kind).join(format!("{}.yaml", crate::store::node_stem(id).0))
    }

    fn row_files(out: &Path, kind: &str) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = crate::vfs::read_dir(Self::row_dir(out, kind))
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension().and_then(|x| x.to_str()) == Some("yaml")
                            && !p
                                .file_name()
                                .map(|n| n.to_string_lossy().starts_with('.'))
                                .unwrap_or(true)
                    })
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        files
    }

    // Every file the ledger reads: what a watcher fingerprints.
    pub fn files(out: &Path) -> Vec<PathBuf> {
        let mut v = vec![Self::path(out)];
        v.extend(Self::row_files(out, "entities"));
        v.extend(Self::row_files(out, "requirements"));
        v
    }

    // Parsed once per content: the status of every row reloads the ledger to see
    // whether its entity was generated here, and a novel's two hundred rows parsed
    // the same file two hundred times, 28 seconds for one Inbox read in a tab. The
    // cache is keyed on the paths and the texts, so any write reads fresh.
    pub fn load(out: &Path) -> Ledger {
        use std::hash::{Hash, Hasher};
        static CACHE: std::sync::Mutex<Option<(PathBuf, u64, Ledger)>> = std::sync::Mutex::new(None);
        let path = Self::path(out);
        let texts: Vec<(PathBuf, String)> = Self::files(out)
            .into_iter()
            .filter_map(|p| crate::vfs::read_to_string(&p).ok().map(|t| (p, t)))
            .collect();
        if texts.is_empty() {
            return Ledger::default();
        }
        // A file that exists but does not parse makes the whole ledger read as empty
        // and read-only; an empty file is no ledger yet.
        let parse = |texts: &[(PathBuf, String)]| -> Ledger {
            let unreadable = |why: String| {
                eprintln!("[jazyk] error: the ledger did not load: {}", why);
                Ledger {
                    unreadable: Some(why),
                    ..Default::default()
                }
            };
            let mut ledger = Ledger::default();
            for (p, text) in texts {
                if text.trim().is_empty() {
                    continue;
                }
                let kind = p
                    .parent()
                    .and_then(|d| d.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if *p == path {
                    match serde_norway::from_str::<Ledger>(text) {
                        // Read first; a version 3 ledger carries its rows inline.
                        Ok(l) => ledger = l,
                        Err(e) => return unreadable(format!("{}: {}", p.display(), e)),
                    }
                    continue;
                }
                let mut v: serde_norway::Value = match serde_norway::from_str(text) {
                    Ok(v) => v,
                    Err(e) => return unreadable(format!("{}: {}", p.display(), e)),
                };
                let id = match &mut v {
                    serde_norway::Value::Mapping(m) => m.remove("id").and_then(|i| i.as_str().map(String::from)),
                    _ => None,
                };
                let Some(id) = id else {
                    return unreadable(format!("{}: a ledger row carries no id", p.display()));
                };
                let read = match kind.as_str() {
                    "entities" => serde_norway::from_value::<EntityGen>(v).map(|r| {
                        ledger.entities.insert(id, r);
                    }),
                    _ => serde_norway::from_value::<ReqRow>(v).map(|r| {
                        ledger.requirements.insert(id, r);
                    }),
                };
                if let Err(e) = read {
                    return unreadable(format!("{}: {}", p.display(), e));
                }
            }
            ledger
        };
        if crate::vfs::is_scoped() {
            return parse(&texts);
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        texts.hash(&mut h);
        let key = h.finish();
        if let Ok(guard) = CACHE.lock() {
            if let Some((p, k, l)) = guard.as_ref() {
                if *p == path && *k == key {
                    return l.clone();
                }
            }
        }
        let ledger: Ledger = parse(&texts);
        if let Ok(mut guard) = CACHE.lock() {
            *guard = Some((path, key, ledger.clone()));
        }
        ledger
    }

    // One file per row and the deliverable-wide file, each through a sibling temporary
    // file renamed into place and only when its bytes changed; the files of rows that
    // left the ledger are deleted. Refused while a file on disk did not parse.
    // Mirrors docs/consumers/gen.md#the-ledger.
    pub fn save(&self, out: &Path) -> Result<(), String> {
        let path = Self::path(out);
        if let Some(why) = &self.unreadable {
            let e = format!("refusing to write {}: the ledger did not load: {}", path.display(), why);
            eprintln!("[jazyk] error: {}", e);
            return Err(e);
        }
        let errors: std::cell::RefCell<Vec<String>> = Default::default();
        let write = |p: &Path, text: &str| {
            if crate::vfs::read_to_string(p).ok().as_deref() == Some(text) {
                return;
            }
            if let Err(e) = crate::store::write_atomic(p, text.as_bytes()) {
                errors.borrow_mut().push(format!("cannot write {}: {}", p.display(), e));
            }
        };
        let row = |id: &str, value: serde_norway::Result<serde_norway::Value>| -> Option<String> {
            let mut m = serde_norway::Mapping::new();
            m.insert("id".into(), id.into());
            if let Ok(serde_norway::Value::Mapping(fields)) = value {
                m.extend(fields);
            }
            serde_norway::to_string(&m).ok()
        };
        let mut written: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
        for (id, r) in &self.entities {
            let p = Self::row_path(out, "entities", id);
            if let Some(t) = row(id, serde_norway::to_value(r)) {
                write(&p, &t);
            }
            written.insert(p);
        }
        for (id, r) in &self.requirements {
            let p = Self::row_path(out, "requirements", id);
            if let Some(t) = row(id, serde_norway::to_value(r)) {
                write(&p, &t);
            }
            written.insert(p);
        }
        let mut wide = serde_norway::to_value(self).unwrap_or(serde_norway::Value::Null);
        if let serde_norway::Value::Mapping(m) = &mut wide {
            m.shift_remove("entities");
            m.shift_remove("requirements");
        }
        match serde_norway::to_string(&wide) {
            Ok(t) => write(&path, &t),
            Err(e) => errors.borrow_mut().push(format!("cannot serialize {}: {}", path.display(), e)),
        }
        for kind in ["entities", "requirements"] {
            for p in Self::row_files(out, kind) {
                if !written.contains(&p) {
                    crate::vfs::remove_file(&p).ok();
                }
            }
        }
        let errors = errors.into_inner();
        if errors.is_empty() {
            Ok(())
        } else {
            let e = errors.join("; ");
            eprintln!("[jazyk] error: the ledger did not save: {}", e);
            Err(format!("the ledger did not save: {}", e))
        }
    }

    // The ledger's writer lock, `<out>/.ledger.lock`: every load, modify, and save
    // runs under it, so two processes never lose each other's rows. Reentrant on one
    // thread; an in-memory tree belongs to one thread and takes no file lock. Never
    // held while a test command runs. Mirrors docs/consumers/gen.md#the-ledger.
    pub fn lock(out: &Path) -> LedgerLock {
        let path = out.join(".ledger.lock");
        let nested = LEDGER_LOCKS.with(|held| {
            let mut held = held.borrow_mut();
            let n = held.entry(path.clone()).or_insert(0);
            *n += 1;
            *n > 1
        });
        #[cfg(not(target_arch = "wasm32"))]
        if !nested && !crate::vfs::is_scoped() {
            crate::vfs::create_dir_all(out).ok();
            let opened = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path);
            match opened {
                Ok(file) => {
                    let started = std::time::Instant::now();
                    let mut warned = false;
                    while fs2::FileExt::try_lock_exclusive(&file).is_err() {
                        if !warned && started.elapsed() >= std::time::Duration::from_secs(10) {
                            eprintln!("[jazyk] waiting for the ledger writer holding {}", path.display());
                            warned = true;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    return LedgerLock { path, file: Some(file) };
                }
                Err(e) => eprintln!("[jazyk] warning: cannot open {}: {}; writing the ledger without the lock", path.display(), e),
            }
        }
        let _ = nested;
        LedgerLock {
            path,
            #[cfg(not(target_arch = "wasm32"))]
            file: None,
        }
    }

    // Load, change, and save under the ledger lock. The change's error, or a failed
    // save, is the result; nothing is reported as recorded that did not land.
    pub fn update<R>(out: &Path, change: impl FnOnce(&mut Ledger) -> Result<R, String>) -> Result<R, String> {
        let _lock = Self::lock(out);
        let mut ledger = Self::load(out);
        let r = change(&mut ledger)?;
        ledger.save(out)?;
        Ok(r)
    }
}

pub struct LedgerLock {
    path: PathBuf,
    #[cfg(not(target_arch = "wasm32"))]
    file: Option<std::fs::File>,
}

thread_local! {
    static LEDGER_LOCKS: std::cell::RefCell<BTreeMap<PathBuf, usize>> = const { std::cell::RefCell::new(BTreeMap::new()) };
}

impl Drop for LedgerLock {
    fn drop(&mut self) {
        LEDGER_LOCKS.with(|held| {
            let mut held = held.borrow_mut();
            if let Some(n) = held.get_mut(&self.path) {
                *n -= 1;
                if *n == 0 {
                    held.remove(&self.path);
                }
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(f) = &self.file {
            fs2::FileExt::unlock(f).ok();
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
        if in_out.vexists() {
            return in_out;
        }
        let in_deliverable = gs.deliverable.join(&test.artifact);
        if in_deliverable.vexists() {
            return in_deliverable;
        }
        in_out
    } else {
        gs.deliverable.join(&test.artifact)
    }
}

// A criteria file is executable judgment setup, not an arbitrary existing file.
pub(crate) fn criteria_front_matter(rid: &str, criterion: Option<&str>, statement: &str) -> String {
    let mut fields = serde_json::Map::new();
    fields.insert("requirement".into(), json!(rid));
    fields.insert("hash".into(), json!(hash_hex(statement)));
    if let Some(cid) = criterion { fields.insert("criterion".into(), json!(cid)); }
    format!("---\n{}---\n", serde_norway::to_string(&fields).expect("criteria metadata serializes"))
}

pub(crate) fn validate_criteria_file(store: &Store, rid: &str, criterion: Option<&str>, path: &Path) -> Result<(), String> {
    use pulldown_cmark::{Event, MetadataBlockKind, Options, Parser, Tag, TagEnd};
    #[derive(Deserialize)]
    struct Header { requirement: String, hash: String, #[serde(default)] criterion: Option<String> }
    let check = || -> Result<(), String> {
        if !path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e, "md" | "markdown")) {
            return Err("use a separate Markdown criteria file, not executable test source".into());
        }
        let content = crate::vfs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut parser = Parser::new_ext(&content, Options::ENABLE_YAML_STYLE_METADATA_BLOCKS).into_offset_iter();
        if !matches!(parser.next(), Some((Event::Start(Tag::MetadataBlock(MetadataBlockKind::YamlStyle)), _))) {
            return Err("YAML front matter is required".into());
        }
        let mut yaml = String::new();
        let mut end = None;
        for (event, range) in parser {
            match event {
                Event::Text(text) => yaml.push_str(&text),
                Event::End(TagEnd::MetadataBlock(_)) => { end = Some(range.end); break; }
                _ => {}
            }
        }
        let header: Header = serde_norway::from_str(&yaml).map_err(|e| format!("invalid criteria metadata: {e}"))?;
        let local_id = |id: &str| -> Result<String, String> {
            let (set, local) = crate::webapi::sets::unqualify(id);
            if set.as_deref().is_some_and(|set| Some(set) != store.out.file_name().and_then(|s| s.to_str())) {
                return Err("criteria IDs belong to a different document set; copy criteriaFrontMatter".into());
            }
            Ok(store.resolve_id(&local).to_string())
        };
        if local_id(&header.requirement)? != rid { return Err(format!("requirement must be {rid}")); }
        let cid = header.criterion.as_deref().map(local_id).transpose()?;
        if criterion.is_some() && cid.as_deref() != criterion {
            return Err(format!("criterion must be {}", criterion.unwrap()));
        }
        let statement = if let Some(cid) = cid.as_deref() {
            &store.graph.criteria.get(cid).filter(|c| c.requirement == rid)
                .ok_or("criterion is not owned by this requirement")?.statement
        } else {
            &store.graph.requirements.get(rid).ok_or("unknown requirement")?.statement
        };
        if header.hash != hash_hex(statement) { return Err("hash must match the current statement; copy criteriaFrontMatter".into()); }
        let body = content.get(end.ok_or("unclosed criteria front matter")?..).unwrap_or_default().trim();
        let words: Vec<_> = body.split(|c: char| !c.is_alphabetic()).collect();
        if body.is_empty() || !["PASS", "FAIL"].iter().all(|expected| words.iter().any(|word| word.eq_ignore_ascii_case(expected))) {
            return Err("the body must contain inspection steps and explicit PASS and FAIL outcomes".into());
        }
        Ok(())
    };
    check().map_err(|e| format!("invalid llm criteria artifact `{}`: {e}. Write a valid criteria file before recording; never substitute a programmatic test file", path.display()))
}

pub fn hash_file(path: &Path) -> String {
    crate::vfs::read(path)
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

// Output is never implementation: a built medium's artifact and every path the build
// produces are rebuilt on every run (a deck's zip carries fresh timestamps), so a row
// listing them would read stale-code after each run and its verify goal would never
// close. Mirrors docs/consumers/gen.md#status-is-derived-never-stored.
pub fn build_outputs(ledger: &Ledger) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    if let Some(m) = &ledger.medium {
        if m.produced == "built" && !m.artifact.trim().is_empty() {
            out.insert(m.artifact.trim().trim_start_matches("./").to_string());
        }
    }
    if let Some(b) = &ledger.build {
        for p in &b.produces {
            out.insert(p.trim().trim_start_matches("./").to_string());
        }
    }
    out
}

// The files of a list that count as implementation: the build outputs set aside.
pub fn implementing_files(ledger: &Ledger, files: &[String]) -> Vec<String> {
    let outputs = build_outputs(ledger);
    files
        .iter()
        .filter(|f| !outputs.contains(f.trim().trim_start_matches("./")))
        .cloned()
        .collect()
}

// The files hash a row's stale-code check compares: implementing files alone.
pub fn hash_implementing(out: &Path, gs: &GenSettings, files: &[String]) -> String {
    hash_files(gs, &implementing_files(&Ledger::load(out), files))
}

// Scope is independent of byte freshness. Test artifacts and build outputs never
// count as implementation, including legacy manifests that named them in files.
pub(crate) fn implementation_outside_source(gs: &GenSettings, ledger: &Ledger, files: &[String]) -> bool {
    let artifacts = test_artifacts(ledger);
    implementing_files(ledger, files).iter().any(|file| {
        !artifacts.contains(file) && !gs.allows_source(file)
    })
}

// A recorded binding must still describe this set's current deliverable. Internal
// LLM criteria remain metadata; external criteria and programmatic tests are Source.
pub(crate) fn binding_source_current(out: &Path, gs: &GenSettings, ledger: &Ledger, row: &ReqRow) -> bool {
    !implementation_outside_source(gs, ledger, &row.files)
        && std::iter::once(&row.test).chain(row.tests.values().map(|test| &test.test)).all(|test| {
            (test.kind == "llm" && artifact_path(out, gs, test).starts_with(out.join("gen")))
                || gs.allows_source(&test.artifact)
        })
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

// A path that restates the deliverable directory's own name (`handbook/hiring.md`
// when the deliverable is `handbook`) while the deliverable holds no directory of
// that name: the path under the root it means (`hiring.md`; empty for the name
// alone). None when the path keeps as given.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
pub fn unroot(deliverable: &Path, rel: &str) -> Option<String> {
    let name = deliverable
        .vcanonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .or_else(|| deliverable.file_name().map(|n| n.to_string_lossy().to_string()))?;
    if name.is_empty() {
        return None;
    }
    let rel = rel.trim().replace('\\', "/");
    let rel = rel.trim_start_matches("./");
    let rest = if rel == name || rel == format!("{}/", name) {
        ""
    } else {
        rel.strip_prefix(&format!("{}/", name))?
    };
    if deliverable.join(&name).vis_dir() {
        return None;
    }
    Some(rest.trim_start_matches('/').to_string())
}

// The reply's note for a path `unroot` moved under the root.
pub fn unroot_note(from: &str, to: &str) -> String {
    format!(
        "`{}` restates the deliverable directory's own name; paths are relative to the deliverable, so this is `{}`. Write paths without that prefix",
        from.trim(),
        if to.is_empty() { "." } else { to }
    )
}

// `confine_rel`, then `unroot`: the path the ledger records, with a restated root
// stripped and noted.
pub fn confine_in(gs: &GenSettings, path: &str, notes: &mut Vec<String>) -> Result<String, String> {
    let rel = confine_rel(path)?;
    let relative=match unroot(&gs.deliverable, &rel) {
        Some(under) if !under.is_empty() => {
            let note = unroot_note(&rel, &under);
            if !notes.contains(&note) {notes.push(note);}
            under
        }
        _ => rel,
    };
    if !gs.allows_source(&relative) {return Err(format!("{} is outside this document set's source selection or belongs to documents",relative));}
    Ok(relative)
}

// A working directory the same way; the restated root alone is the deliverable.
pub fn confine_cwd_in(gs: &GenSettings, cwd: &str, notes: &mut Vec<String>) -> Result<String, String> {
    let c = confine_cwd(cwd)?;
    if c == "." {
        return Ok(c);
    }
    match unroot(&gs.deliverable, &c) {
        Some(under) => {
            let note = unroot_note(&c, &under);
            if !notes.contains(&note) {
                notes.push(note);
            }
            Ok(if under.is_empty() { ".".into() } else { under })
        }
        None => Ok(c),
    }
}

// Every path a list names, confined, deduplicated in first-seen order.
fn confine_list_in(
    gs: &GenSettings,
    paths: &[String],
    notes: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for p in paths {
        out.push(confine_in(gs, p, notes)?);
    }
    Ok(dedup_keep_order(out))
}

// Every test artifact the ledger's rows name, headline and criterion tests alike.
// They are the rows', never an entity's implementation.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
pub fn test_artifacts(ledger: &Ledger) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for row in ledger.requirements.values() {
        out.insert(row.test.artifact.clone());
        for t in row.tests.values() {
            out.insert(t.test.artifact.clone());
        }
    }
    out.remove("");
    out
}

// The entity outside `id`'s component group that records `path`, by slug. Members
// of one group share their files; everyone else owns theirs alone. `id` may be a
// requirement (a bind goal's target), which belongs to no group, so every recorded
// file is someone else's.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
pub fn file_owner(store: &Store, ledger: &Ledger, id: &str, path: &str) -> Option<String> {
    if ledger.support.iter().any(|support| support == path) {
        return None;
    }
    let id = store.resolve_id(id);
    let group = group_root(store, id);
    ledger
        .entities
        .iter()
        .find(|(slug, e)| {
            let other = format!("ent:{}", slug);
            let other = store.resolve_id(&other);
            other != id && group_root(store, other) != group && e.files.iter().any(|f| f == path)
        })
        .map(|(slug, _)| slug.clone())
}

// The other members of `id`'s component group, the entities whose files it shares.
// A system, and an entity outside every group, shares with nobody.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
pub fn shared_with(store: &Store, id: &str) -> Vec<String> {
    let id = store.resolve_id(id);
    if is_system(store, id) {
        return Vec::new();
    }
    group_members(store, &group_root(store, id))
        .into_iter()
        .filter(|m| m != id)
        .collect()
}

// A working directory the same way, where `.` (or nothing) means the deliverable.
pub fn confine_cwd(cwd: &str) -> Result<String, String> {
    let c = cwd.trim();
    if c.is_empty() || c == "." || c == "./" {
        return Ok(".".into());
    }
    confine_rel(c)
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

// The suggested test name of a criterion: `crit_<slug>_<hash8>`, the hash of the
// criterion statement (docs/consumers/gen.md#tests-tie-criteria-to-the-deliverable).
pub fn crit_test_name(cid: &str, statement: &str) -> String {
    let sanitized: String = cid
        .strip_prefix("crit:")
        .unwrap_or(cid)
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("crit_{}_{}", sanitized, &hash_hex(statement)[..8])
}

pub fn reqs_of_sorted(store: &Store, id: &str) -> Vec<String> {
    let mut v = store.requirements_referencing(id);
    v.sort();
    v
}

// What `reqs_of_sorted` answers for every entity, built in one pass over the
// requirements: the requirement ids naming the entity (redirects followed), sorted.
// A scan over the entities that asked per entity read the five thousand requirements
// of a large store two thousand times.
pub struct ReqsByEntity(BTreeMap<String, Vec<String>>);

impl ReqsByEntity {
    pub fn build(store: &Store) -> ReqsByEntity {
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (rid, r) in &store.graph.requirements {
            let mut named: Vec<&str> = Vec::new();
            for e in &r.entities {
                let id = store.resolve_id(e);
                if !named.contains(&id) {
                    named.push(id);
                    map.entry(id.to_string()).or_default().push(rid.clone());
                }
            }
        }
        for v in map.values_mut() {
            v.sort();
        }
        ReqsByEntity(map)
    }

    pub fn of(&self, id: &str) -> &[String] {
        self.0.get(id).map(Vec::as_slice).unwrap_or(&[])
    }
}

// The facts generation consumes, folded into one hash: name, definition,
// stereotype, attributes, and every referencing statement with its edges. Any of
// them changing flips the entity to generation work.
// Mirrors docs/consumers/gen.md#the-ledger.
pub fn fact_hash(store: &Store, id: &str) -> String {
    fact_hash_over(store, id, &reqs_of_sorted(store, id))
}

// `fact_hash` with the entity's sorted requirement ids already in hand.
pub fn fact_hash_over(store: &Store, id: &str, rids: &[String]) -> String {
    assets::fingerprint(&fact_hash_base(store, id, rids), &assets::referenced_assets(store, id, rids))
}

fn fact_hash_base(store: &Store, id: &str, rids: &[String]) -> String {
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
    for rid in rids {
        if let Some(r) = store.graph.requirements.get(rid) {
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
        let Ok(rd) = crate::vfs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<crate::vfs::DirEntry> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if out.len() >= 40 {
                return;
            }
            let p = e.path();
            let Ok(relative) = p.strip_prefix(root) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            if !crate::document_sets::safe_relative(root, &relative) {
                continue;
            }
            if p.vis_dir() {
                walk(&p, root, out, depth + 1);
            } else {
                out.push(relative);
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
#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub async fn decide_medium(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
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
        ).await?;
        // A rate limit relayed as a reply is the endpoint's answer, not a decision:
        // say so instead of blaming an empty field (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
        if crate::reconcile::rate_limited(&reply) {
            return Err(format!(
                "could not decide the deliverable's medium: the model call was rate limited ({})",
                crate::llm::truncate(reply.trim(), 200)
            ));
        }
        let excerpt = crate::llm::truncate(reply.trim(), 120).to_string();
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
            last = format!(
                "`produced` was `{}`, not `written` or `built` (the reply began: {})",
                produced, excerpt
            );
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
pub fn contract() -> String {
    include_str!("../../docs/compiler/goals/prompts/generate-contract.md")
        .replace("{GROUP}", &GROUP.to_string())
}

pub fn instructions() -> String {
    contract() + "\n\n" + crate::task_runtime::PROMPT
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
// (`files-gone`), a binding is incomplete, or a bound requirement is unimplemented.
// An entity with no entry and only current adopted rows is never generated over.
// Mirrors docs/compiler/goals/generate.md#created-when.
pub fn pending(store: &Store, gs: &GenSettings) -> Vec<Value> {
    let ledger = Ledger::load(&store.out);
    let binding_owed: std::collections::BTreeSet<String> = crate::bind::pending(store, gs)
        .iter().filter_map(|row| row["requirement"].as_str().map(String::from)).collect();
    let by_entity = ReqsByEntity::build(store);
    let scan = assets::AssetScan::default();
    let mut out = Vec::new();
    for id in store.graph.entities.keys() {
        let rids = by_entity.of(id);
        if rids.is_empty() {
            continue;
        }
        let slug = slug_of(id);
        let referenced_assets = assets::referenced_assets_in(&scan, store, id, rids);
        let hash = assets::fingerprint(&fact_hash_base(store, id, rids), &referenced_assets);
        let incomplete: Vec<&String> = rids.iter().filter(|rid| binding_owed.contains(*rid)).collect();
        let unimplemented: Vec<&String> = rids
            .iter()
            .filter(|rid| {
                ledger
                    .requirements
                    .get(*rid)
                    .map(|row| matches!(crate::verify::status_with(store, rid, row, gs, &ledger).0.as_str(), "unimplemented" | "partial"))
                    .unwrap_or(false)
            })
            .collect();
        let (reason, changed) = match ledger.entities.get(&slug) {
            Some(e) if e.fact_hash != hash => {
                let (r, c) = change_diff(&ledger, &slug, rids);
                (r, json!(c))
            }
            Some(e) => {
                let gone: Vec<&String> = e
                    .files
                    .iter()
                    .filter(|f| !gs.deliverable.join(f).vexists())
                    .collect();
                // A record with no files is an entity carried by other entities' files
                // (an actor whose statements the tool implements); nothing of its own
                // can be gone. Mirrors docs/compiler/goals/generate.md#created-when.
                if implementation_outside_source(gs, &ledger, &e.files) {
                    ("source-selection-changed".to_string(), json!(e.files))
                } else if !gone.is_empty() {
                    ("files-gone".to_string(), json!(gone))
                } else if !unimplemented.is_empty() {
                    ("unimplemented-bindings".to_string(), json!(unimplemented))
                } else if !incomplete.is_empty() {
                    ("incomplete-bindings".to_string(), json!(incomplete))
                } else {
                    continue;
                }
            }
            None if !unimplemented.is_empty() => {
                ("unimplemented-bindings".to_string(), json!(unimplemented))
            }
            None if !incomplete.is_empty() => {
                ("incomplete-bindings".to_string(), json!(incomplete))
            }
            None if referenced_assets.iter().any(|asset| asset.is_input()) => {
                ("referenced-inputs".to_string(), json!(referenced_assets.iter()
                    .filter(|asset| asset.is_input()).map(|asset| &asset.path).collect::<Vec<_>>()))
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

// The generate goal's gate: record_generation landed for the entity with a factHash
// equal to the live graph's and files that exist; with `require_run`, run_tests ran
// afterwards too (no programmatic row of the entity is still never-run). Ok carries
// the recorded file count. Err names what is missing and the call that lands it.
// Mirrors docs/compiler/goals/generate.md#gate.
pub fn gate(store: &Store, gs: &GenSettings, id: &str, require_run: bool) -> Result<usize, String> {
    let id = store.resolve_id(id).to_string();
    if !store.graph.entities.contains_key(&id) {
        return Err(format!("`{}` is not in the graph; nothing generates it", id));
    }
    assets::ensure_available(&assets::referenced_assets(store, &id, &reqs_of_sorted(store, &id)))?;
    let ledger = Ledger::load(&store.out);
    let Some(e) = ledger.entities.get(&slug_of(&id)) else {
        return Err(format!(
            "no ledger entry for `{}`: record_generation has not landed; the gate is record_generation with the package's factHash, then run_tests",
            id
        ));
    };
    let live = fact_hash(store, &id);
    if e.fact_hash != live {
        return Err(format!(
            "the record for `{}` carries factHash {} but the graph says {}: the facts moved since the package was built; begin_generation again and record_generation with the current factHash",
            id, e.fact_hash, live
        ));
    }
    if implementation_outside_source(gs, &ledger, &e.files) {
        return Err(format!(
            "the record for `{}` names implementing files outside current Source; re-bind its requirements and record_generation inside the current source selection",
            id
        ));
    }
    let gone: Vec<&String> = e
        .files
        .iter()
        .filter(|f| !gs.deliverable.join(f).vis_file())
        .collect();
    if !gone.is_empty() {
        return Err(format!(
            "the record for `{}` lists files that do not exist under the deliverable ({}); write them, then record_generation again with every file the manifest names",
            id,
            gone.iter().map(|f| f.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    // A record with no implementing file of its own stands only when every
    // requirement of the entity has a row naming an implementing file that exists:
    // the entity is carried by other entities' files. Test artifacts and criteria
    // files are not implementation. Mirrors docs/compiler/goals/generate.md#gate.
    let artifacts = test_artifacts(&ledger);
    let outputs = build_outputs(&ledger);
    let implementing = |f: &String| {
        !artifacts.contains(f) && !outputs.contains(f) && gs.allows_source(f) && gs.deliverable.join(f).vis_file()
    };
    if !e.files.iter().any(&implementing) {
        let missing: Vec<String> = reqs_of_sorted(store, &id)
            .into_iter()
            .filter(|rid| {
                !ledger
                    .requirements
                    .get(rid)
                    .is_some_and(|row| row.files.iter().any(&implementing))
            })
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "the record for `{}` lists no implementing file, and these requirements have no row naming a file that realizes them: {}; write the entity's part and record it in files",
                id,
                missing.join(", ")
            ));
        }
    }
    let missing_tests: Vec<String> = reqs_of_sorted(store, &id).into_iter()
        .filter(|rid| crate::bind::gate(store, gs, rid).is_err()).collect();
    if !missing_tests.is_empty() {
        return Err(format!(
            "required test bindings are missing or stale for {}; record_generation with each current requirement and criterion test",
            missing_tests.join(", ")
        ));
    }
    if require_run {
        let never_run: Vec<String> = reqs_of_sorted(store, &id)
            .into_iter()
            .filter(|rid| {
                ledger
                    .requirements
                    .get(rid)
                    .map(|row| {
                        (row.test.kind == "programmatic"
                            && crate::verify::status_with(store, rid, row, gs, &ledger)
                                == ("unverified".to_string(), "never-run".to_string()))
                            || row.tests.values().any(|test| test.test.kind == "programmatic"
                                && (test.verdict == "none" || test.last_run.is_none()))
                    })
                    .unwrap_or(false)
            })
            .collect();
        if !never_run.is_empty() {
            return Err(format!(
                "run_tests has not run since the record: {} has incomplete programmatic evidence; call run_tests, then mark the goal",
                never_run.join(", ")
            ));
        }
    }
    Ok(e.files.len())
}

// Why `jazyk gen` leaves an entity alone, for the person reading the trace: the
// entity is current, or its adopted rows need no generation.
// Mirrors docs/consumers/gen.md#incremental-regeneration.
pub fn skip_reason(store: &Store, gs: &GenSettings, id: &str) -> String {
    if let Some(row) = pending(store, gs).iter().find(|row| row["entity"] == id) {
        return format!("generation work remains ({})", row["reason"].as_str().unwrap_or("incomplete"));
    }
    let ledger = Ledger::load(&store.out);
    if ledger.entities.contains_key(&slug_of(id)) {
        "unchanged".to_string()
    } else {
        "no ledger entry and all adopted rows are current: there is nothing to generate (`--force` generates it anyway)".to_string()
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
        if candidate.vis_file() {
            let rel = norm_rel(
                &pathdiff(&candidate, &gs.deliverable).unwrap_or_else(|| token.to_string()),
            );
            let content = crate::vfs::read_to_string(&candidate).unwrap_or_default();
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
    let referenced_assets = assets::referenced_assets(store, id, &rids);
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
                            "criteriaFrontMatter": criteria_front_matter(rid, None, &r.statement),
                            "testName": test_name(rid, &r.statement),
                            // The criteria with the names binding gives their tests,
                            // so a test generation must write (a requirement with no
                            // bound row) carries the name binding would look for.
                            // Mirrors docs/compiler/goals/generate.md#what-the-model-sees.
                            "criteria": store.criteria_of(rid).iter().filter_map(|cid| {
                                let c = store.graph.criteria.get(cid)?;
                                Some(json!({
                                    "id": cid, "statement": c.statement,
                                    "suggestedTestName": crit_test_name(cid, &c.statement),
                                    "criteriaFrontMatter": criteria_front_matter(rid, Some(cid), &c.statement),
                                }))
                            }).collect::<Vec<Value>>(),
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
                        crate::vfs::read_to_string(gs.deliverable.join(f))
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
        "sourceSelection": gs.code,
        "factHash": assets::fingerprint(&fact_hash_base(store, id, &rids), &referenced_assets),
        "referencedAssets": referenced_assets,
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
        "mediumNote": medium_note(&ledger),
        "build": ledger.build.as_ref().map(|b| json!({"run": b.run, "cwd": b.cwd, "produces": b.produces})),
        // The tests binding already wrote: they define the interface, and the product
        // conforms to them. A task that cannot make one pass without changing it
        // reports that; the repair is a re-bind
        // (docs/consumers/bind.md#generation-makes-bound-tests-pass).
        // Shared requirements confused a Codex session into re-recording them
        // (docs/compiler/goals/generate.md#what-the-model-sees).
        "ownershipNote": "a requirement already carried by another entity's file names that file on its test row (files: [path]); an entity whose every requirement is carried records files: []; extend a sharedWith member's file and record it in files. A genuine shared composition entrypoint can be support even for a written artifact: begin its recorded owner, preserve its content/current files/tests and re-record with that entrypoint added to supportFiles, then begin your entity and extend it. Also list it in files when it implements your requirements. Never promote an exclusive module merely to bypass ownership",
        // The entity's component group: members whose files it may write and record
        // too (docs/consumers/gen.md#file-ownership-and-conventions).
        "sharedWith": shared_with(store, id),
        "boundTests": rids
            .iter()
            .filter_map(|rid| {
                ledger.requirements.get(rid).map(|row| {
                    let (status, _) = crate::verify::status_with(store, rid, row, gs, &ledger);
                    json!({
                        "requirement": rid,
                        "status": status,
                        "files": row.files,
                        "test": {"kind": row.test.kind, "artifact": row.test.artifact,
                                 "name": row.test.name, "run": row.test.run, "cwd": row.test.cwd},
                        "criteriaTests": row.tests.iter().map(|(cid, t)| json!({
                            "criterion": cid, "kind": t.test.kind, "artifact": t.test.artifact,
                            "name": t.test.name, "run": t.test.run, "cwd": t.test.cwd, "verdict": t.verdict,
                        })).collect::<Vec<Value>>(),
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
    let rids = reqs_of_sorted(store, id);
    let referenced_assets = assets::referenced_assets(store, id, &rids);
    assets::ensure_available(&referenced_assets)?;
    let base_hash = fact_hash_base(store, id, &rids);
    let current_hash = assets::fingerprint(&base_hash, &referenced_assets);
    if current_hash != base_hash && fact_hash_seen.is_some_and(|seen| seen != current_hash) {
        return Err("The facts or referenced input bytes moved since begin_generation. Reload the package, use the current exact inputs and record its current factHash.".into());
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
    // A path that restates the deliverable directory's own name is moved under the
    // root, and the reply says so (docs/consumers/gen.md#file-ownership-and-conventions).
    let mut path_notes: Vec<String> = Vec::new();
    let files: Vec<String> = confine_list_in(gs, &str_list(&manifest["files"]), &mut path_notes)
        .map_err(|e| format!("files: {}", e))?;
    // Support files are the deliverable's, recorded once and rewritable by any later
    // task (docs/consumers/gen.md#file-ownership-and-conventions).
    let support: Vec<String> =
        confine_list_in(gs, &str_list(&manifest["supportFiles"]), &mut path_notes)
            .map_err(|e| format!("supportFiles: {}", e))?;
    let build_produces: Vec<String> =
        confine_list_in(gs, &str_list(&manifest["build"]["produces"]), &mut path_notes)
            .map_err(|e| format!("build.produces: {}", e))?;
    let build_cwd = confine_cwd_in(gs, manifest["build"]["cwd"].as_str().unwrap_or("."), &mut path_notes)
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
    let mut rows: Vec<(String, Option<String>, TestRef, Vec<String>)> = Vec::new();
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
                let named = [&t["name"], &t["artifact"], &t["label"]]
                    .iter()
                    .find_map(|v| v.as_str().filter(|s| !s.trim().is_empty()))
                    .unwrap_or("?")
                    .to_string();
                return Err(format!(
                    "test row `{}` names no requirement; every row carries the id of the requirement it tests ({{requirement, kind, label, artifact, name, run, cwd}})",
                    named
                ));
            };
            let rid = store.resolve_id(rid).to_string();
            let Some(r) = store.graph.requirements.get(&rid) else {
                return Err(format!("unknown requirement `{}` in manifest", rid));
            };
            let criterion = t["criterion"].as_str().filter(|id| !id.is_empty())
                .map(|id| store.resolve_id(id).to_string());
            if let Some(cid) = &criterion {
                if !store.graph.criteria.get(cid).is_some_and(|criterion| criterion.requirement == rid) {
                    return Err(format!("criterion `{cid}` is not a criterion of `{rid}`"));
                }
            }
            if rows.iter().any(|(previous, cid, _, _)| previous == &rid && cid == &criterion) {
                return Err(format!("duplicate test declaration for `{rid}`{}", criterion.as_deref().map(|cid| format!(" criterion `{cid}`")).unwrap_or_default()));
            }
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
                        "test row for {} has an empty artifact; name the test source the row's run command executes, not its generated JUnit report",
                        rid
                    ));
                }
                if run.is_empty() {
                    return Err(format!(
                        "test row for {} has an empty run command; record task:<id> with its case selector, or a legacy command selecting that test",
                        rid
                    ));
                }
            } else if artifact_raw.is_empty() {
                return Err(format!(
                    "test row for {} is llm but names no criteria file; write it (the package names its path under criteria/) and record that path as the artifact",
                    rid
                ));
            }
            let artifact = confine_in(gs, &artifact_raw, &mut path_notes)
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
                cwd: confine_cwd_in(gs, t["cwd"].as_str().unwrap_or("."), &mut path_notes)
                    .map_err(|e| format!("test row for {}: cwd {}", rid, e))?,
            };
            let path = artifact_path(&store.out, gs, &test);
            if !path.vis_file() {
                return Err(format!(
                    "test row for {} names artifact `{}` but no such file exists under the deliverable{}; write its test source or criteria file before recording, or fix the path. Generated JUnit reports belong in task reports, not artifact",
                    rid,
                    test.artifact,
                    if kind == "llm" { " or the out directory's gen/" } else { "" }
                ));
            }
            if kind == "programmatic"
                && !crate::verify::artifact_names(&path, &test.name)
            {
                return Err(format!(
                    "test row for {} declares test `{}` but `{}` does not contain that name; artifact must name the test source, not the generated JUnit report; match name to the test in that source and the emitted report case",
                    rid, test.name, test.artifact
                ));
            }
            if kind == "llm" { validate_criteria_file(store, &rid, criterion.as_deref(), &path)?; }
            // The row's own `files`; an empty list takes the default below.
            let row_files: Vec<String> =
                confine_list_in(gs, &str_list(&t["files"]), &mut path_notes)
                    .map_err(|e| format!("test row for {}: files {}", rid, e))?;
            rows.push((rid, criterion, test, row_files));
        }
    }
    // Ownership and realization, both before any side effect.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    let _lock = Ledger::lock(&store.out);
    let pre = Ledger::load(&store.out);
    let record_sequence = pre.entities.get(&slug).map(|row| row.record_sequence).unwrap_or(0)
        .checked_add(1).ok_or("The entity generation record sequence is exhausted; no record was changed.")?;
    for (rid, criterion, test, _) in &rows {
        let Some(cid) = criterion else {
            if let Some(row) = pre.requirements.get(rid) {
                let path = artifact_path(&store.out, gs, &row.test);
                if row.hashes.requirement == hash_hex(&store.graph.requirements[rid].statement)
                    && row.hashes.test == hash_file(&path) && path.vis_file()
                    && ((row.test.kind == "llm" && path.starts_with(store.out.join("gen"))) || gs.allows_source(&row.test.artifact))
                    && !same_test_artifact(&row.test, test) {
                    return Err(format!("requirement `{rid}` already has a current bound test; reuse its artifact, name, and kind instead of replacing the judge"));
                }
            }
            continue;
        };
        let Some(prior) = pre.requirements.get(rid).and_then(|row| row.tests.get(cid)) else { continue };
        let path = artifact_path(&store.out, gs, &prior.test);
        let current = prior.criterion_hash == hash_hex(&store.graph.criteria[cid].statement)
            && prior.test_hash == hash_file(&path) && path.vis_file()
            && ((prior.test.kind == "llm" && path.starts_with(store.out.join("gen"))) || gs.allows_source(&prior.test.artifact))
            && (prior.test.kind != "programmatic" || crate::verify::artifact_names(&path, &prior.test.name));
        if current && !same_test_artifact(&prior.test, test) {
            return Err(format!("criterion `{cid}` already has a current bound test; reuse its artifact, name, and kind instead of replacing the judge"));
        }
    }
    let mut artifacts = test_artifacts(&pre);
    for (_, _, t, _) in &rows {
        artifacts.insert(t.artifact.clone());
    }
    let outputs = build_outputs(&pre);
    // Implementation: a file that exists and is no test artifact, criteria file, or
    // build output.
    let implementing = |f: &String| {
        !artifacts.contains(f)
            && !outputs.contains(f)
            && !build_produces.contains(f)
            && gs.allows_source(f)
            && gs.deliverable.join(f).vis_file()
    };
    // A file an entity outside this entity's component group records is theirs. Test
    // artifacts are the rows' and support files the deliverable's: neither conflicts.
    for f in &files {
        if artifacts.contains(f) || support.contains(f) || pre.support.contains(f) {
            continue;
        }
        if let Some(owner) = file_owner(store, &pre, id, f) {
            return Err(format!(
                "files: `{}` belongs to entity `{}`, outside this entity's component group; never record another group's file. Write a file of your own for your requirements, or name `{}` on a test row's `files` when it already carries that requirement",
                f, owner, f
            ));
        }
    }
    // A row naming no files of its own takes the entity's implementing files, and
    // keeps the files other entities record for it: a shared requirement stays
    // carried where it is (docs/consumers/gen.md#file-ownership-and-conventions).
    let own_impl: Vec<String> = files.iter().filter(|f| implementing(f)).cloned().collect();
    let rows: Vec<(String, Option<String>, TestRef, Vec<String>)> = rows
        .into_iter()
        .map(|(rid, criterion, test, own)| {
            if !own.is_empty() {
                return (rid, criterion, test, own);
            }
            let mut v = own_impl.clone();
            if let Some(prior) = pre.requirements.get(&rid) {
                for f in &prior.files {
                    let carried = pre
                        .entities
                        .iter()
                        .any(|(s, e)| s != &slug && e.files.contains(f));
                    if carried && implementing(f) && !v.contains(f) {
                        v.push(f.clone());
                    }
                }
            }
            (rid, criterion, test, v)
        })
        .collect();
    // A record realizes the entity's requirements in files: with no implementing file
    // of its own, every requirement needs a row naming one, this manifest's or the
    // ledger's (an entity carried by other entities' files).
    if own_impl.is_empty() {
        let uncarried: Vec<String> = reqs_of_sorted(store, id)
            .into_iter()
            .filter(|rid| {
                let named = match rows.iter().find(|(r, _, _, _)| r == rid) {
                    Some((_, _, _, fs)) => fs.iter().any(&implementing),
                    None => pre
                        .requirements
                        .get(rid)
                        .is_some_and(|row| row.files.iter().any(&implementing)),
                };
                !named
            })
            .collect();
        if !uncarried.is_empty() {
            return Err(format!(
                "files names no file that realizes {}'s requirements (test and criteria files are not implementation), and nothing else carries {}; write the part of the deliverable that realizes them and list it in files, or, where a file another entity recorded already carries a requirement, name that file on the requirement's test row (`files: [path]`). Only an entity whose every requirement is carried by another entity's files records files: []",
                id,
                uncarried.join(", ")
            ));
        }
    }
    // Under a built medium the build is the only thing that makes an artifact exist,
    // so a manifest without one is rejected, not quietly accepted
    // (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
    // Every rejection runs before the strip below rewrites files: a rejected record
    // leaves the markers in place for the retry.
    let manifest_build = manifest["build"]["run"].as_str().is_some_and(|r| !r.trim().is_empty());
    if pre.build.is_none() && !manifest_build {
        if let Some(m) = pre.medium.as_ref().filter(|m| m.is_built()) {
            return Err(format!(
                "this deliverable is {} and no build is recorded: return `build` with the command that produces `{}`, its `cwd`, and `produces` listing `{}`",
                m.form, m.artifact, m.artifact
            ));
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
        regex::Regex::new(r"req:(?:ds_[A-Za-z0-9_-]+::)?[A-Za-z0-9][A-Za-z0-9_-]*\s+hash:[0-9a-fA-F]{4,}").unwrap();
    let mut marker_warnings: Vec<String> = Vec::new();
    for f in &files {
        let path = gs.deliverable.join(f);
        let Ok(text) = crate::vfs::read_to_string(&path) else {
            continue;
        };
        let (clean, raw) = strip_markers_in(&text, store.out.file_name().and_then(|s| s.to_str()));
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
        // Through a sibling temporary file renamed into place: a crash mid-strip
        // never leaves a truncated source file.
        crate::store::write_atomic(&path, clean.as_bytes()).map_err(|e| e.to_string())?;
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
            // A built medium without a build was rejected before the strip.
            None => {}
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
            tasks: str_list(&manifest["tasks"]),
            unattached: None,
            recorded_at: Some(crate::verify::now_iso()),
            record_sequence,
        },
    );
    let mut grouped: BTreeMap<String, Vec<(Option<String>, TestRef, Vec<String>)>> = BTreeMap::new();
    for (rid, criterion, test, row_files) in rows {
        grouped.entry(rid).or_default().push((criterion, test, row_files));
    }
    let mut seeded = 0;
    // Rows landing over an open error diagnostic: named in the reply and kept in
    // the ledger, so generation never runs green over a contradiction silently.
    // Mirrors docs/consumers/gen.md#rows-recorded-over-an-open-contradiction.
    let mut contradicted: BTreeMap<String, Vec<String>> = BTreeMap::new();
    {
        for (rid, declarations) in grouped {
            let r = &store.graph.requirements[&rid];
            let previous = ledger.requirements.get(&rid).cloned();
            let mut tests = previous.as_ref().map(|row| row.tests.clone()).unwrap_or_default();
            tests.retain(|cid, _| store.graph.criteria.get(cid).is_some_and(|criterion| criterion.requirement == rid));
            let mut headline = None;
            let mut row_files = Vec::new();
            for (criterion, test, files) in declarations {
                for file in files {
                    if !row_files.contains(&file) { row_files.push(file); }
                }
                if let Some(cid) = criterion {
                    let criterion_hash = hash_hex(&store.graph.criteria[&cid].statement);
                    let test_hash = hash_file(&artifact_path(&store.out, gs, &test));
                    let existing = tests.get(&cid).filter(|old| old.criterion_hash == criterion_hash
                        && old.test_hash == test_hash && same_test_ref(&old.test, &test));
                    let value = existing.cloned().unwrap_or_else(|| CritTest {
                        test, criterion_hash, test_hash, verdict:"none".into(), last_run:None,
                        exit_code:None, evidence:None,
                    });
                    tests.insert(cid, value);
                } else {
                    headline = Some(test);
                }
            }
            let test = headline.or_else(|| tests.values().next().map(|test| test.test.clone()))
                .expect("each grouped row contains a test declaration");
            let hashes = RowHashes {
                requirement: hash_hex(&r.statement),
                test: hash_file(&artifact_path(&store.out, gs, &test)),
                files: hash_files(gs, &implementing_files(&ledger, &row_files)),
            };
            let owner = r
                .entities
                .first()
                .map(|e| store.resolve_id(e).to_string())
                .unwrap_or_else(|| id.to_string());
            // A re-record that changes nothing keeps its verdict: the hashes say the
            // requirement, the test, and the files are the very state the last run
            // judged, and discarding that judgment would punish an idempotent call.
            let prior = previous.as_ref()
                .filter(|row| row.hashes == hashes && same_test_ref(&row.test, &test));
            let (verdict, last_run, exit_code, evidence) = match prior {
                Some(row) => (
                    row.verdict.clone(),
                    row.last_run.clone(),
                    row.exit_code,
                    row.evidence.clone(),
                ),
                None => ("none".into(), None, None, None),
            };
            // A declaration cannot carry an old verdict onto different source or
            // requirement bytes. The next execution or judgment supplies evidence.
            if previous.as_ref().is_some_and(|row| row.hashes.files != hashes.files
                || row.hashes.requirement != hashes.requirement) {
                for test in tests.values_mut() {
                    test.verdict = "none".into();
                    test.last_run = None;
                    test.exit_code = None;
                    test.evidence = None;
                }
            }
            ledger.requirements.insert(
                rid.clone(),
                ReqRow {
                    tests,
                    entity: owner,
                    files: row_files,
                    // An unchanged support promotion has no markers left to strip.
                    sites: sites_by_rid.remove(&rid)
                        .unwrap_or_else(|| prior.map(|row| row.sites.clone()).unwrap_or_default()),
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
        if claimed || !gs.allows_source(f) || !gs.deliverable.join(f).vis_file() {
            continue;
        }
        snapshot_baseline(&store.out, gs, f, &mut baselined);
        if crate::vfs::remove_file(gs.deliverable.join(f)).is_ok() {
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
    ledger.save(&store.out)?;
    let mut reply = json!({
        "marked": id, "files": files.len(), "tests": seeded,
        "unattached": {"files": unattached.files, "lines": unattached.lines, "ratio": unattached.ratio, "markerless": unattached.markerless},
        "unattachedNote": if unattached.markerless {
            "informational: no requirement site anchors any line of these files (the medium carries no traceability markers), so the ratio says nothing here; the generation is accepted (docs/consumers/gen.md#the-unattached-remainder)"
        } else {
            "informational: the owned files no requirement row names and the significant lines outside every recorded site; the generation is accepted, and a large ratio suggests the deliverable states behavior the documents do not (docs/consumers/gen.md#the-unattached-remainder)"
        },
    });
    if !path_notes.is_empty() {
        reply["pathNote"] = json!(path_notes);
    }
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
        let Ok(text) = crate::vfs::read_to_string(gs.deliverable.join(f)) else {
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
    let markerless = significant > 0 && e.files.iter().all(|f| !first_site.contains_key(f.as_str()));
    Unattached {
        files,
        lines,
        ratio,
        markerless,
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
                    facts: Vec::new(),
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
            Some("incomplete-bindings") => json!("incomplete-bindings"),
            Some("referenced-inputs") => json!("referenced-inputs"),
            _ => json!("facts-changed"),
        };
        push(
            &mut index,
            p["entity"].as_str().unwrap_or_default().to_string(),
            json!({"goal": "generate", "reason": reason, "changed": p["changed"]}),
        );
    }
    for p in crate::verify::pending(store, gs, Some("stale"), None) {
        if p["status"] == "unimplemented" || p["status"] == "partial" || p["reason"] == "not-generated" {
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
    use crate::runner::SessionRunner;

    #[test]
    fn task_build_requires_promised_products() {
        let _scope = crate::vfs::Scope::enter(&Default::default());
        let out = Path::new("/project/.jazyk");
        let (_, gs) = fixture(out);
        let build = Build { run: "task:build".into(), cwd: ".".into(),
            produces: vec!["product.txt".into()], last_run: None };
        let mut ledger = Ledger::load(out);
        ledger.build = Some(build.clone());
        ledger.save(out).unwrap();
        assert!(check_build_products(out, &gs, &build).unwrap_err().contains("did not produce product.txt"));
        assert!(!Ledger::load(out).build.unwrap().last_run.unwrap().ok);
        crate::vfs::write(gs.deliverable.join("product.txt"), "ready").unwrap();
        check_build_products(out, &gs, &build).unwrap();
        assert!(Ledger::load(out).build.unwrap().last_run.unwrap().ok);
    }

    // The one-pass requirement index answers what the per-entity scan answers, for
    // every entity, and the fact hash over it is the fact hash.
    #[test]
    fn reqs_by_entity_matches_the_per_entity_scan() {
        let mut synthetic = crate::derive::tests::synthetic_store(300, 700);
        synthetic
            .graph
            .redirects
            .insert("ent:old".into(), "ent:e1".into());
        synthetic.graph.requirements.insert(
            "req:redirected".into(),
            crate::model::Requirement {
                statement: "The old one, twice.".into(),
                entities: vec!["ent:old".into(), "ent:e1".into(), "ent:e2".into()],
                ..Default::default()
            },
        );
        for s in [synthetic, crate::board::tests::settled_store()] {
            let index = ReqsByEntity::build(&s);
            assert!(index.of("ent:nope").is_empty());
            for id in s.graph.entities.keys() {
                assert_eq!(index.of(id), reqs_of_sorted(&s, id).as_slice(), "{}", id);
                assert_eq!(fact_hash_over(&s, id, index.of(id)), fact_hash(&s, id), "{}", id);
            }
        }
    }

    #[test]
    fn an_unparseable_ledger_reads_empty_and_refuses_to_save() {
        let out = std::env::temp_dir().join(format!(
            "jazyk-gen-unreadable-{}",
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&out).ok();
        let path = Ledger::path(&out);
        let mut l = Ledger::default();
        l.support.push("package.json".to_string());
        l.save(&out).unwrap();
        assert!(Ledger::load(&out).support.contains(&"package.json".to_string()));
        let broken = "support: [unterminated\n";
        crate::vfs::write(&path, broken).unwrap();
        let mut bad = Ledger::load(&out);
        assert!(bad.unreadable.as_deref().is_some_and(|w| w.contains("ledger.yaml")));
        assert!(bad.support.is_empty());
        bad.support.push("other".to_string());
        assert!(bad.save(&out).is_err(), "an unreadable ledger refuses to save");
        assert_eq!(crate::vfs::read_to_string(&path).unwrap(), broken);
        let leftover: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftover.is_empty(), "{:?}", leftover);
    }
    use crate::model::*;

    // A runner that stops without recording, and one that records: gen counts an
    // entity as generated only when its record moved during the session, and names
    // the stop otherwise. Mirrors docs/consumers/gen.md#the-ledger.
    struct StubRunner {
        record: bool,
        failed: Option<String>,
        stopped: Option<String>,
        store: Store,
        gs: GenSettings,
    }
    impl crate::runner::SessionRunner for StubRunner {
        fn agent_name(&self) -> String {
            "stub".into()
        }
        fn set_build_token(&self, _token: Option<String>) {}
        fn run_item(&self, _batch: &crate::runner::BatchRun, _trace: &crate::session::Trace) -> crate::runner::SessionReport {
            if self.record {
                crate::vfs::write(self.gs.deliverable.join("cart.rs"), "fn cart() {}\n").unwrap();
                crate::vfs::create_dir_all(self.gs.deliverable.join("tests")).unwrap();
                crate::vfs::write(self.gs.deliverable.join("tests/cart.rs"), "fn cart_test() {}\n").unwrap();
                mark(&self.store, "ent:cart", None, &json!({"files": ["cart.rs"], "tests": [{
                    "requirement":"req:shop-1","kind":"programmatic","artifact":"tests/cart.rs",
                    "name":"cart_test","run":"cargo test cart_test"
                }]}), &self.gs).unwrap();
            }
            crate::runner::SessionReport { applied: 0, rounds: 1, tokens: 0, cached: 0, failed: self.failed.clone(), resolved: Vec::new(), stopped: self.stopped.clone() }
        }
        fn ask(&self, _s: &str, _u: &str, _l: &str, _st: &str) -> Result<String, String> {
            Err("stub".into())
        }
        fn ask_traced(&self, _s: &str, _u: &str, _l: &str, _st: &str, _t: Option<&crate::session::Trace>) -> Result<String, String> {
            Err("stub".into())
        }
    }

    #[test]
    fn a_session_that_recorded_nothing_is_no_generation() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-landed-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        crate::vfs::create_dir_all(&gs.deliverable).unwrap();
        // A record from an earlier run stands in the ledger.
        crate::vfs::write(gs.deliverable.join("cart.rs"), "fn old() {}\n").unwrap();
        mark(&s, "ent:cart", None, &json!({"files": ["cart.rs"], "tests": []}), &gs).unwrap();
        let trace = crate::session::Trace::to_sink(crate::session::TraceLevel::Normal, std::sync::Arc::new(|_| {}), Default::default());
        let ids = vec!["ent:cart".to_string()];
        let idle = StubRunner { failed: None, record: false, stopped: Some("http error: HTTP 429 :: rate limit".into()), store: s.clone(), gs: gs.clone() };
        let r = gen_session(&s, &idle, &gs, "ent:cart", &ids, &trace);
        let err = r.results[0].1.as_ref().unwrap_err();
        assert!(err.contains("429"), "{}", err);
        let quiet = StubRunner { failed: None, record: false, stopped: None, store: s.clone(), gs: gs.clone() };
        let r = gen_session(&s, &quiet, &gs, "ent:cart", &ids, &trace);
        assert!(r.results[0].1.as_ref().unwrap_err().contains("before recording completion"), "{:?}", r.results[0].1);
        let writer = StubRunner { failed: None, record: true, stopped: None, store: s.clone(), gs: gs.clone() };
        let r = gen_session(&s, &writer, &gs, "ent:cart", &ids, &trace);
        assert_eq!(r.results[0].1.as_ref().unwrap(), &1, "{:?}", r.results[0].1);
        assert!(Ledger::load(&out).entities["cart"].recorded_at.is_some());
        crate::vfs::remove_dir_all(&out).ok();
    }

    #[test]
    fn repeated_generation_in_the_same_timestamp_second_proves_completion() {
        let _scope = crate::vfs::Scope::enter(&BTreeMap::new());
        let out = Path::new("/project/.jazyk/same-second");
        let (store, gs) = fixture(out);
        let writer = StubRunner { record:true, failed:None, stopped:None, store:store.clone(), gs:gs.clone() };
        let trace = crate::session::Trace::to_sink(crate::session::TraceLevel::Normal,
            std::sync::Arc::new(|_| {}), Default::default());
        let batch = crate::runner::BatchRun { id:"same-second".into(), goals:vec![], executor:None };
        writer.run_item(&batch, &trace);
        let timestamp = "2026-10-30T12:00:00Z";
        let mut ledger = Ledger::load(out);
        ledger.entities.get_mut("cart").unwrap().recorded_at = Some(timestamp.into());
        ledger.save(out).unwrap();
        assert_eq!(ledger.entities["cart"].record_sequence, 1);

        struct SameSecondRunner(StubRunner);
        impl crate::runner::SessionRunner for SameSecondRunner {
            fn agent_name(&self) -> String { self.0.agent_name() }
            fn set_build_token(&self, token: Option<String>) { self.0.set_build_token(token); }
            fn run_item(&self, batch: &crate::runner::BatchRun, trace: &crate::session::Trace) -> crate::runner::SessionReport {
                let report = self.0.run_item(batch, trace);
                // Pin the display time after a real successful mark. The two
                // records deterministically share a second without sleeping.
                let mut ledger = Ledger::load(&self.0.store.out);
                ledger.entities.get_mut("cart").unwrap().recorded_at = Some("2026-10-30T12:00:00Z".into());
                ledger.save(&self.0.store.out).unwrap();
                report
            }
            fn ask(&self, system: &str, user: &str, label: &str, stage: &str) -> Result<String, String> {
                self.0.ask(system, user, label, stage)
            }
            fn ask_traced(&self, system: &str, user: &str, label: &str, stage: &str, trace: Option<&crate::session::Trace>) -> Result<String, String> {
                self.0.ask_traced(system, user, label, stage, trace)
            }
        }
        let result = gen_session(&store, &SameSecondRunner(writer), &gs, "ent:cart", &["ent:cart".into()], &trace);
        assert!(result.results[0].1.is_ok(), "{:?}", result.results);
        let after = Ledger::load(out);
        assert_eq!(after.entities["cart"].record_sequence, 2);
        assert_eq!(after.entities["cart"].recorded_at.as_deref(), Some(timestamp));
    }

    #[test]
    fn old_generation_checkpoint_and_ledger_keep_their_completion_semantics() {
        let old = json!({
            "session":{"batch":{"id":"g:generate:ent:cart","goals":[],"executor":null},"generation":1,"report":null},
            "before":["2026-10-30T12:00:00Z"],"result":null
        });
        let mut state: GenSessionState = serde_json::from_value(old.clone()).unwrap();
        assert!(state.before_sequences.is_none());
        assert_eq!(serde_json::to_value(&state).unwrap(), old);
        let mut row: EntityGen = serde_json::from_value(json!({
            "factHash":"old", "recordedAt":"2026-10-30T12:00:00Z"
        })).unwrap();
        assert_eq!(row.record_sequence, 0);
        assert!(serde_json::to_value(&row).unwrap().get("recordSequence").is_none());
        row.record_sequence = 1;
        assert!(!state.recorded(0, Some(&row)), "old sessions still compare their admitted timestamps");
        row.recorded_at = Some("2026-10-30T12:00:01Z".into());
        assert!(state.recorded(0, Some(&row)));
        state.before_sequences = Some(vec![Some(1)]);
        assert!(!state.recorded(0, Some(&row)), "new sessions ignore display time changes");
        row.record_sequence = 2;
        assert!(state.recorded(0, Some(&row)));
    }

    #[test]
    fn exhausted_generation_receipt_refuses_before_mutating_source_or_ledger() {
        let _scope = crate::vfs::Scope::enter(&BTreeMap::new());
        let out = Path::new("/project/.jazyk/exhausted-generation-receipt");
        let (store, gs) = fixture(out);
        let file = gs.deliverable.join("cart.rs");
        crate::vfs::write(&file, "fn cart() {}\n").unwrap();
        let manifest = json!({"files":["cart.rs"],"tests":[]});
        mark(&store, "ent:cart", None, &manifest, &gs).unwrap();
        let mut ledger = Ledger::load(out);
        ledger.entities.get_mut("cart").unwrap().record_sequence = u64::MAX;
        ledger.save(out).unwrap();
        let before = crate::vfs::read(Ledger::path(out)).unwrap();
        let source = "// req:shop-1 hash:abcdef12\nfn cart() {}\n";
        crate::vfs::write(&file, source).unwrap();
        assert!(mark(&store, "ent:cart", None, &manifest, &gs).unwrap_err().contains("sequence is exhausted"));
        assert_eq!(crate::vfs::read_to_string(&file).unwrap(), source);
        assert_eq!(crate::vfs::read(Ledger::path(out)).unwrap(), before);
    }

    #[test]
    fn a_fresh_generation_manifest_wins_over_a_later_runtime_error() {
        let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
        let out = std::path::Path::new("/project/.jazyk/post-record");
        let (mut store, gs) = fixture(out);
        store.graph.entities.insert("ent:other".into(), Entity { name:"Other".into(), ..Default::default() });
        let trace = crate::session::Trace::to_sink(crate::session::TraceLevel::Normal,
            std::sync::Arc::new(|_| {}), Default::default());
        let writer = StubRunner { record:true, failed:Some("the model call failed: connection closed".into()),
            stopped:Some("the model call failed: connection closed".into()), store:store.clone(), gs:gs.clone() };
        let result = gen_session(&store, &writer, &gs, "ent:cart", &["ent:cart".into()], &trace);
        assert!(result.results[0].1.is_ok());
        assert!(result.failed.is_none());
        let mut ledger = Ledger::load(out);
        ledger.entities.get_mut("cart").unwrap().recorded_at = Some("before-partial".into());
        ledger.save(out).unwrap();
        let result = gen_session(&store, &writer, &gs, "ent:cart", &["ent:cart".into(), "ent:other".into()], &trace);
        assert!(result.results[0].1.is_ok());
        assert_eq!(result.results[1].1.as_ref().unwrap_err(), "the model call failed: connection closed");
        assert_eq!(result.failed.as_deref(), Some("the model call failed: connection closed"));
    }

    #[test]
    fn a_generation_runtime_error_does_not_attempt_the_next_group() {
        use crate::runner::{BatchRun, SessionReport, SessionRunner};
        struct Broken(std::cell::Cell<usize>);
        impl SessionRunner for Broken {
            fn agent_name(&self) -> String { "broken".into() }
            fn set_build_token(&self, _: Option<String>) {}
            fn run_item(&self, _: &BatchRun, _: &crate::session::Trace) -> SessionReport {
                self.0.set(self.0.get() + 1);
                SessionReport { applied:0,rounds:1,tokens:7,cached:0,failed:Some("the model call failed: disconnected".into()),resolved:Vec::new(),stopped:Some("the model call failed: disconnected".into()) }
            }
            fn ask(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String,String> {
                self.0.set(self.0.get()+1);
                Err("the model call failed: disconnected".into())
            }
            fn ask_traced(&self, _: &str, _: &str, _: &str, _: &str, _: Option<&crate::session::Trace>) -> Result<String,String> { unreachable!() }
        }
        for worker in ["agentic", "pipeline"] {
        let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
        let out = std::path::Path::new("/project/.jazyk/generation-fatal");
        let (mut store, mut gs) = fixture(out);
        gs.worker = worker.into();
        store.graph.entities.insert("ent:other".into(), Entity { name:"Other".into(), ..Default::default() });
        store.graph.requirements.insert("req:other-1".into(), Requirement {
            statement:"Other holds items.".into(),entities:vec!["ent:other".into()],..Default::default()
        });
        crate::vfs::create_dir_all(&gs.deliverable).unwrap();
        crate::vfs::write(gs.deliverable.join("cart.rs"), "fn previous() {}\n").unwrap();
        mark(&store,"ent:cart",None,&json!({"files":["cart.rs"],"tests":[]}),&gs).unwrap();
        let mut ledger = Ledger::load(out);
        ledger.medium=Some(Medium { form:"Rust source".into(),produced:"written".into(),toolchain:"rustc".into(),artifact:String::new() });
        ledger.save(out).unwrap();
        let runner = Broken(std::cell::Cell::new(0));
        let trace = crate::session::Trace::to_sink(crate::session::TraceLevel::Normal,
            std::sync::Arc::new(|_|{}),Default::default());
        let result = run_selected(&store,&runner,&gs,&[],true,&trace,false).unwrap();
        assert_eq!(runner.0.get(),1);
        assert_eq!(result["failures"],1);
        assert_eq!(result["stopped"],"the model call failed: disconnected");
        assert!(!Ledger::load(out).entities.contains_key("other"));
        }
    }

    #[test]
    fn an_incomplete_group_keeps_completed_work_and_skips_later_groups_and_build() {
        struct Partial {
            writer: StubRunner,
            calls: std::cell::Cell<usize>,
            stopped: Option<String>,
        }
        impl crate::runner::SessionRunner for Partial {
            fn agent_name(&self) -> String { "partial".into() }
            fn set_build_token(&self, _: Option<String>) {}
            fn run_item(&self, batch: &crate::runner::BatchRun, trace: &crate::session::Trace) -> crate::runner::SessionReport {
                self.calls.set(self.calls.get() + 1);
                match batch.goals[0].target.as_str() {
                    "ent:cart" => self.writer.run_item(batch, trace),
                    "ent:middle" => {
                        crate::vfs::write(self.writer.gs.deliverable.join("index.html"), "<html>partial shared page").unwrap();
                        let mut ledger = Ledger::load(&self.writer.store.out);
                        ledger.support.push("index.html".into());
                        ledger.build = Some(Build { run:"printf built > build-was-run.txt".into(),
                            cwd:".".into(), produces:vec!["build-was-run.txt".into()], last_run:None });
                        ledger.save(&self.writer.store.out).unwrap();
                        crate::runner::SessionReport { applied:0,rounds:24,tokens:20,cached:0,
                            failed:None,resolved:vec![],stopped:self.stopped.clone() }
                    }
                    other => panic!("later group {other} consumed a partial shared page"),
                }
            }
            fn ask(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String,String> { panic!("no new model request") }
            fn ask_traced(&self, _: &str, _: &str, _: &str, _: &str, _: Option<&crate::session::Trace>) -> Result<String,String> { panic!("no new model request") }
        }
        for (case, (later, stopped)) in [(false,None),(true,None),
            (false,Some("the session ran out of rounds after 24".into())),
            (true,Some("the session ran out of rounds after 24".into()))].into_iter().enumerate() {
            let _scope = crate::session_recovery::Scope::enter(None);
            let out = std::env::temp_dir().join(format!("jazyk-gen-partial-stop-{}-{case}", crate::sys::process_id()));
            crate::vfs::remove_dir_all(&out).ok();
            let (mut store, gs) = fixture(&out);
            for (id, name) in [("middle", "Middle"), ("z-last", "Last")] {
                if id == "z-last" && !later { continue; }
                store.graph.entities.insert(format!("ent:{id}"), Entity { name:name.into(), ..Default::default() });
                store.graph.requirements.insert(format!("req:{id}"), Requirement {
                    statement:format!("{name} has content."), entities:vec![format!("ent:{id}")], ..Default::default() });
            }
            let mut ledger = Ledger::load(&out);
            ledger.medium = Some(Medium { form:"Rust source".into(),produced:"written".into(),toolchain:"rustc".into(),artifact:String::new() });
            ledger.save(&out).unwrap();
            let runner = Partial { writer:StubRunner { record:true,failed:None,stopped:None,
                store:store.clone(),gs:gs.clone() }, calls:std::cell::Cell::new(0), stopped:stopped.clone() };
            let trace = crate::session::Trace::to_sink(crate::session::TraceLevel::Quiet, std::sync::Arc::new(|_|{}), Default::default());
            let result = run_selected(&store, &runner, &gs, &[], true, &trace, true).unwrap();
            assert_eq!(runner.calls.get(), 2, "{result}");
            assert_eq!(result["regenerated"], 1);
            assert_eq!(result["failures"], 1);
            assert_eq!(result["build"]["state"], "not-run");
            let error = result["error"].as_str().unwrap();
            assert!(error.contains("group Middle") && error.contains("Still owed: Middle"), "{error}");
            assert!(error.contains(stopped.as_deref().unwrap_or("before recording completion")), "{error}");
            let ledger = Ledger::load(&out);
            assert!(ledger.entities.contains_key("cart") && !ledger.entities.contains_key("middle"));
            assert!(crate::vfs::read_to_string(gs.deliverable.join("cart.rs")).unwrap().contains("fn cart"));
            assert_eq!(crate::vfs::read_to_string(gs.deliverable.join("index.html")).unwrap(), "<html>partial shared page");
            assert!(!gs.deliverable.join("build-was-run.txt").exists());
            assert!(ledger.build.unwrap().last_run.is_none());
            crate::vfs::remove_dir_all(&out).ok();
        }
    }

    #[test]
    fn prior_generation_group_progress_keeps_its_position_without_an_error_field() {
        let old = json!({"root":"ent:cart","ready":["ent:cart","ent:other"],
            "pipeline_cursor":1,"pipeline_task":null,"pipeline_results":[["ent:cart",{"Ok":1}]]});
        let restored: GenGroup = serde_json::from_value(old).unwrap();
        assert_eq!(restored.pipeline_cursor,1);
        assert_eq!(restored.pipeline_results,vec![("ent:cart".into(),Ok(1))]);
        assert!(restored.pipeline_failed.is_none());
    }

    #[test]
    fn incomplete_generation_exposes_its_group_and_round_limit_and_stops() {
        let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
        let out = std::path::Path::new("/project/.jazyk/generation-budget");
        let (store,gs) = fixture(out);
        crate::vfs::create_dir_all(&gs.deliverable).unwrap();
        crate::vfs::write(gs.deliverable.join("cart.rs"),"fn prior() {}\n").unwrap();
        mark(&store,"ent:cart",None,&json!({"files":["cart.rs"],"tests":[]}),&gs).unwrap();
        let mut ledger = Ledger::load(out);
        ledger.medium=Some(Medium {form:"Rust".into(),produced:"written".into(),toolchain:"rustc".into(),artifact:String::new()});
        ledger.save(out).unwrap();
        let runner = StubRunner { record:false,failed:None,stopped:Some("the session ran out of rounds after 24".into()),store:store.clone(),gs:gs.clone() };
        let trace = crate::session::Trace::to_sink(crate::session::TraceLevel::Quiet,std::sync::Arc::new(|_|{}),Default::default());
        let result = run_selected(&store,&runner,&gs,&[],true,&trace,false).unwrap();
        assert_eq!(result["failures"],1);
        assert_eq!(result["stopped"], result["error"]);
        let error = result["error"].as_str().unwrap();
        assert!(error.contains("Cart was not completed") && error.contains("24"), "{result}");
        assert!(!error.contains("ent:cart") && !error.contains("record_generation"), "{result}");
        let mut old = serde_json::to_value(GenCommand::default()).unwrap();
        old.as_object_mut().unwrap().remove("first_error");
        let restored:GenCommand = serde_json::from_value(old).unwrap();
        assert!(restored.first_error.is_none());
    }

    #[test]
    fn a_resumed_generation_command_keeps_its_group_and_counts_the_result_once() {
        let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
        let _scope = crate::session_recovery::Scope::enter(None);
        let out = std::path::Path::new("/project/.jazyk/main");
        let (store, gs) = fixture(out);
        crate::vfs::create_dir_all(&gs.deliverable).unwrap();
        crate::vfs::write(gs.deliverable.join("cart.rs"), "fn previous() {}\n").unwrap();
        mark(&store, "ent:cart", None, &json!({"files":["cart.rs"],"tests":[]}), &gs).unwrap();
        let mut ledger = Ledger::load(out);
        ledger.entities.get_mut("cart").unwrap().recorded_at = Some("before-command".into());
        ledger.medium = Some(Medium { form: "Rust source".into(), produced: "written".into(),
            toolchain: "rustc".into(), artifact: String::new() });
        ledger.save(out).unwrap();
        let trace = crate::session::Trace::to_sink(crate::session::TraceLevel::Normal,
            std::sync::Arc::new(|_| {}), Default::default());
        let paused = StubRunner { failed: None, record: false, stopped: Some("checkpoint-failed: test storage outage".into()),
            store: store.clone(), gs: gs.clone() };
        let error = run_selected(&store, &paused, &gs, &[], true, &trace, false).unwrap_err();
        assert!(error.starts_with("checkpoint-failed:"));
        let writer = StubRunner { failed: None, record: true, stopped: None, store: store.clone(), gs: gs.clone() };
        let result = run_selected(&store, &writer, &gs, &[], true, &trace, false).unwrap();
        assert_eq!(result["regenerated"], 1);
        assert_eq!(result["failures"], 0);
        // A worker that would interrupt cannot be reached after the result receipt.
        assert_eq!(run_selected(&store, &paused, &gs, &[], true, &trace, false).unwrap(), result);
    }

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
            configured: true,
            deliverable: out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        (s, gs)
    }

    #[test]
    fn source_scaffolding_can_be_written_bound_and_recorded() {
        let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
        let root = Path::new("/project");
        crate::vfs::create_dir_all(root.join(".jazyk")).unwrap();
        crate::vfs::write(root.join(crate::document_sets::CONFIG), "version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Site'\ndocs = ['**', '!deliverable/**']\nsource = ['deliverable/**']\n").unwrap();
        let project = crate::document_sets::Workspace::load(root).unwrap().resolve(Some("ds_main")).unwrap();
        let (store, _) = fixture(&project.out);
        let gs = GenSettings::resolve(&project);
        let mut tools = crate::tools::ToolSession::new(store.clone(), crate::tools::WorkScope::serving("generate"), 64, 24_000);
        tools.gen = gs.clone();
        let workflow = "deliverable/.github/workflows/deploy.yml";
        let ignore = "deliverable/.gitignore";
        let artifact = "deliverable/tests/hosting.py";
        for (path, content) in [
            (workflow, "name: Deploy\non:\n  push:\n    branches: [master]\njobs: {}\n"),
            (ignore, "build/\n"),
            (artifact, "def test_workflow():\n    pass\n"),
        ] {
            tools.dispatch("write_text_file", &json!({"path":path,"content":content})).unwrap();
            assert_eq!(tools.dispatch("read_text_file", &json!({"path":path})).unwrap()["content"], content);
        }
        let files = tools.dispatch("list_files", &json!({})).unwrap();
        assert!(files["files"].as_array().unwrap().contains(&json!(workflow)));
        assert!(files["files"].as_array().unwrap().contains(&json!(ignore)));
        assert!(list_existing_files(&root.join("deliverable")).contains(&".github/workflows/deploy.yml".into()));
        assert!(list_existing_files(&root.join("deliverable")).contains(&".gitignore".into()));
        assert!(crate::document_sets::Workspace::load(root).unwrap().files().contains(&workflow.to_string()));
        let test = json!({"kind":"programmatic","artifact":artifact,"name":"test_workflow","run":"task:check"});
        crate::bind::record(&store, "req:shop-1", &[workflow.into()], &test, &json!([]), "fail", Some("Static fixture, no deployment executed."), &gs).unwrap();
        crate::bind::gate(&store, &gs, "req:shop-1").unwrap();
        mark(&store, "ent:cart", None, &json!({"files":[workflow],"supportFiles":[ignore],"tests":[{"requirement":"req:shop-1","kind":"programmatic","artifact":artifact,"name":"test_workflow","run":"task:check"}]}), &gs).unwrap();
        assert_eq!(gate(&store, &gs, "ent:cart", false).unwrap(), 1);
        let ledger = Ledger::load(&store.out);
        assert_eq!(ledger.entities["cart"].files, [workflow]);
        assert!(ledger.support.contains(&ignore.to_string()));
        assert!(!implementation_outside_source(&gs, &ledger, &ledger.entities["cart"].files));
        for path in ["deliverable/.env", "deliverable/.github/.env", "deliverable/.ssh/config", "deliverable/.aws/config", "deliverable/.git/config", "deliverable/.jazyk/config"] {
            let err = tools.dispatch("write_text_file", &json!({"path":path,"content":"must not land"})).unwrap_err();
            assert_eq!(err.rule, "source-selection", "{path}");
            assert!(!root.join(path).vexists(), "{path}");
        }
        crate::vfs::create_dir_all(root.join("deliverable/nested/.jazyk")).unwrap();
        crate::vfs::write(root.join("deliverable/nested/.jazyk/jazyk.toml"), "version = 1\n").unwrap();
        crate::vfs::create_dir_all(root.join("deliverable/nested/.github/workflows")).unwrap();
        crate::vfs::write(root.join("deliverable/nested/.github/workflows/own.yml"), "nested project\n").unwrap();
        let err = tools.dispatch("write_text_file", &json!({"path":"deliverable/nested/.github/workflows/new.yml","content":"must not land"})).unwrap_err();
        assert_eq!(err.rule, "source-selection");
        let files = tools.dispatch("list_files", &json!({"path":"deliverable"})).unwrap();
        assert!(!files["files"].as_array().unwrap().iter().any(|path|path.as_str().unwrap().contains("nested/")));
    }

    #[test]
    fn fresh_generation_records_criterion_tests_without_binding_and_keeps_them_unjudged() {
        let out = std::env::temp_dir().join(format!("jazyk-adaptive-criteria-{}",crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (mut store, gs) = fixture(&out);
        for (id, statement) in [("crit:items","Two items are retained."),("crit:empty","An empty cart has no items.")] {
            store.graph.criteria.insert(id.into(), crate::model::Criterion {
                requirement:"req:shop-1".into(),statement:statement.into(),kind:"positive".into(),..Default::default()
            });
        }
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        crate::vfs::write(gs.deliverable.join("cart.rs"),"pub fn count() -> usize { 2 }\n").unwrap();
        crate::vfs::write(gs.deliverable.join("tests/cart.rs"),"fn items() {}\nfn empty() {}\nfn replacement() {}\n").unwrap();
        assert!(Ledger::load(&out).requirements.is_empty());
        assert_eq!(pending(&store,&gs)[0]["reason"],"incomplete-bindings");
        let declaration = |criterion:&str,name:&str| json!({"requirement":"req:shop-1","criterion":criterion,
            "kind":"programmatic","artifact":"tests/cart.rs","name":name,"run":"task:old-check"});
        let mut manifest=json!({"files":["cart.rs","tests/cart.rs"],"tests":[declaration("crit:items","items")]});
        mark(&store,"ent:cart",None,&manifest,&gs).unwrap();
        assert!(gate(&store,&gs,"ent:cart",false).unwrap_err().contains("test bindings"));
        manifest["tests"]=json!([declaration("crit:items","items"),declaration("crit:empty","empty")]);
        mark(&store,"ent:cart",None,&manifest,&gs).unwrap();
        let ledger=Ledger::load(&out);
        let row=&ledger.requirements["req:shop-1"];
        assert_eq!(row.tests.len(),2);
        assert!(row.tests.values().all(|test|test.verdict=="none"&&test.last_run.is_none()));
        assert_eq!(gate(&store,&gs,"ent:cart",false).unwrap(),2);
        assert!(gate(&store,&gs,"ent:cart",true).unwrap_err().contains("run_tests"));
        // A replacement judge is refused before changing files or the ledger.
        let before=serde_json::to_value(&ledger).unwrap();
        let mut replaced=manifest.clone();replaced["tests"][0]["name"]=json!("replacement");
        assert!(mark(&store,"ent:cart",None,&replaced,&gs).unwrap_err().contains("replacing the judge"));
        assert_eq!(serde_json::to_value(Ledger::load(&out)).unwrap(),before);
        // Consolidating execution does not replace the judge, but loses old evidence.
        let mut ledger=Ledger::load(&out);
        let row=ledger.requirements.get_mut("req:shop-1").unwrap();
        row.verdict="pass".into();row.last_run=Some("previous".into());
        for test in row.tests.values_mut(){test.verdict="pass".into();test.last_run=Some("previous".into());}
        ledger.save(&out).unwrap();
        for declaration in manifest["tests"].as_array_mut().unwrap(){declaration["run"]=json!("task:shared-suite");}
        mark(&store,"ent:cart",None,&manifest,&gs).unwrap();
        let ledger=Ledger::load(&out);let row=&ledger.requirements["req:shop-1"];
        assert_eq!(row.verdict,"none");
        assert!(row.tests.values().all(|test|test.test.run=="task:shared-suite"&&test.verdict=="none"&&test.last_run.is_none()));
        assert!(gate(&store,&gs,"ent:cart",true).is_err());
        crate::vfs::remove_dir_all(&out).unwrap();
    }

    #[test]
    fn generated_llm_criterion_waits_for_review_without_blocking_generation() {
        let out=std::env::temp_dir().join(format!("jazyk-adaptive-llm-{}",crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let(mut store,gs)=fixture(&out);
        store.graph.criteria.insert("crit:items".into(),crate::model::Criterion {
            requirement:"req:shop-1".into(),statement:"The cart clearly shows its items.".into(),kind:"positive".into(),..Default::default()
        });
        crate::vfs::create_dir_all(gs.deliverable.join("criteria")).unwrap();
        crate::vfs::write(gs.deliverable.join("cart.md"),"The visible cart contains its selected items.\n").unwrap();
        let criteria=criteria_front_matter("req:shop-1",Some("crit:items"),&store.graph.criteria["crit:items"].statement)
            +"\nInspect cart.md. PASS when its items are clear. FAIL otherwise.\n";
        crate::vfs::write(gs.deliverable.join("criteria/cart.md"),criteria).unwrap();
        mark(&store,"ent:cart",None,&json!({"files":["cart.md"],"tests":[{
            "requirement":"req:shop-1","criterion":"crit:items","kind":"llm","artifact":"criteria/cart.md","name":"clarity","run":"llm"
        }]}),&gs).unwrap();
        assert!(gate(&store,&gs,"ent:cart",true).is_ok());
        let row=&Ledger::load(&out).requirements["req:shop-1"];
        assert_eq!(row.verdict,"none");assert_eq!(row.tests["crit:items"].verdict,"none");
        assert_ne!(crate::verify::status_of(&store,"req:shop-1",row,&gs).0,"verified");
        crate::vfs::remove_dir_all(&out).unwrap();
    }

    #[test]
    fn gdk_develops_fresh_source_and_tests_before_execution() {
        use crate::harness::{self, GenericTool, LoopArgs, ModelHost, ModelRequest, ModelResponse};
        struct Script { replies:std::collections::VecDeque<Value>, calls:usize }
        impl ModelHost for Script {
            fn infer<'a>(&'a mut self, request:ModelRequest)->harness::LocalFuture<'a,Result<ModelResponse,String>> {
                Box::pin(async move {
                    assert!(matches!(request.purpose,harness::ModelPurpose::Inference));
                    self.calls+=1;
                    Ok(ModelResponse { body:self.replies.pop_front().expect("no additional model call is needed") })
                })
            }
        }
        let out=std::env::temp_dir().join(format!("jazyk-gdk-code-first-{}",crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let(mut store,gs)=fixture(&out);
        for(id,statement)in[("crit:items","Two items remain in the cart."),("crit:empty","An empty cart contains zero items.")] {
            store.graph.criteria.insert(id.into(),crate::model::Criterion {
                requirement:"req:shop-1".into(),statement:statement.into(),kind:"positive".into(),..Default::default()
            });
        }
        assert!(Ledger::load(&out).requirements.is_empty());
        assert_eq!(pending(&store,&gs)[0]["reason"],"incomplete-bindings");
        let goal=crate::model::Goal {id:"g:generate:ent:cart".into(),kind:"generate".into(),target:"ent:cart".into(),
            mandatory:true,class:"compile".into(),..Default::default()};
        let scope=crate::tools::WorkScope::for_batch(&goal.id,std::slice::from_ref(&goal));
        crate::vfs::create_dir_all(&gs.deliverable).unwrap();
        let mut session=crate::tools::ToolSession::new(store.clone(),scope,50,32000);
        session.gen=gs.clone();
        let source="class Cart:\n    def __init__(self, items):\n        self.items = list(items)\n    def count(self):\n        return len(self.items)\n";
        let tests="import sys\nfrom pathlib import Path\nsys.path.insert(0, str(Path(__file__).resolve().parents[1]))\nfrom cart import Cart\ndef items():\n    assert Cart(['a', 'b']).count() == 2\ndef empty():\n    assert Cart([]).count() == 0\nglobals()[sys.argv[1]]()\n";
        let manifest=json!({"files":["cart.py","tests/check.py"],"tests":[
            {"requirement":"req:shop-1","criterion":"crit:items","kind":"programmatic","artifact":"tests/check.py","name":"items","run":"python3 tests/check.py items"},
            {"requirement":"req:shop-1","criterion":"crit:empty","kind":"programmatic","artifact":"tests/check.py","name":"empty","run":"python3 tests/check.py empty"}
        ]});
        let actions=[
            ("write_text_file",json!({"path":"cart.py","content":source})),
            ("write_text_file",json!({"path":"tests/check.py","content":tests})),
            ("record_generation",json!({"entity":"ent:cart","factHash":fact_hash(&store,"ent:cart"),"manifest":manifest})),
            ("run_tests",json!({"requirements":["req:shop-1"]})),
            ("mark_goal_done",json!({"goal":goal.id,"justification":"The implementation retains items and both criterion tests passed against it."})),
            ("done",json!({"summary":"Implemented the cart and executed its criterion tests."})),
        ];
        let replies=actions.iter().enumerate().map(|(i,(name,args))|json!({
            "choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{
                "id":format!("adaptive-{i}"),"type":"function","function":{"name":name,"arguments":args.to_string()}
            }]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}
        })).collect();
        let mut model=Script {replies,calls:0};
        let names:std::collections::BTreeSet<_>=actions.iter().map(|(name,_)|*name).collect();
        let tools:Vec<_>=crate::tools::catalog().into_iter().filter(|tool|names.contains(tool.name)).map(|tool|GenericTool {
            name:tool.name.into(),description:tool.description.into(),parameters:tool.parameters,
        }).collect();
        let llm=crate::llm::Llm { api: "chat_completions".into(), reasoning_summary: None, base_url:"http://127.0.0.1:1/v1".into(),model:"scripted-native-gdk".into(),
            api_key:String::new(),temperature:None,trace:None};
        let mut history=vec![json!({"role":"user","content":"Develop the cart source and its criterion tests together."})];
        let mut observed=Vec::new();
        let mut dispatch=|name:&str,args:&Value| {
            if name=="run_tests" {
                assert_eq!(crate::vfs::read_to_string(gs.deliverable.join("cart.py")).unwrap(),source);
                assert!(gs.deliverable.join("tests/check.py").vis_file());
                assert!(gate(&store,&gs,"ent:cart",true).is_err(),"declarations cannot claim execution");
            }
            observed.push(name.to_string());
            let value=session.dispatch(name,args).unwrap_or_else(|error|panic!("fixture {name}: {}",error.message));
            Ok(value.to_string())
        };
        let stopped=harness::run_with_host(LoopArgs {llm:&llm,history:&mut history,tools:&tools,dispatch:&mut dispatch,
            emit:&mut |_|{},cancelled:&||false,max_rounds:8,label:"code-first".into(),conversation:false,inputs:None,input_close:None},
            &instructions(),&mut model,&mut harness::MemoryRecovery);
        assert_eq!(stopped,harness::Stop::EndTurn);
        assert_eq!(observed,actions.iter().map(|(name,_)|name.to_string()).collect::<Vec<_>>());
        assert_eq!(model.calls,6,"successful completion needs no extra inference");
        assert!(gate(&store,&gs,"ent:cart",true).is_ok());
        let row=&Ledger::load(&out).requirements["req:shop-1"];
        assert_eq!(row.verdict,"pass");
        assert!(row.tests.values().all(|test|test.verdict=="pass"&&test.last_run.is_some()));
        assert_eq!(crate::verify::status_of(&store,"req:shop-1",row,&gs).0,"verified");
        assert!(!observed.iter().any(|name|name=="record_binding"||name=="begin_binding"));
        crate::vfs::remove_dir_all(&out).unwrap();
    }

    #[test]
    fn recorded_medium_is_not_decided_again_before_the_first_entity_generation() {
        let out=std::env::temp_dir().join(format!("jazyk-medium-once-{}",crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let(store,gs)=fixture(&out);
        Ledger { medium:Some(Medium {form:"Rust crate".into(),produced:"written".into(),
            toolchain:"cargo".into(),artifact:String::new()}),..Default::default() }.save(&out).unwrap();
        assert!(Ledger::load(&out).entities.is_empty());
        let runner=StubRunner {record:true,failed:None,stopped:None,store:store.clone(),gs:gs.clone()};
        let trace=crate::session::Trace::to_sink(crate::session::TraceLevel::Quiet,std::sync::Arc::new(|_|{}),Default::default());
        // StubRunner refuses every ask: deciding again would fail this run.
        let result=run_selected(&store,&runner,&gs,&["ent:cart".into()],false,&trace,false).unwrap();
        assert_eq!(result["regenerated"],1,"{result}");
        assert_eq!(result["failures"],0,"{result}");
        assert_eq!(Ledger::load(&out).medium.unwrap().form,"Rust crate");
        crate::vfs::remove_dir_all(&out).unwrap();
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
    fn qualified_source_markers_anchor_only_their_document_set() {
        let _scope = crate::vfs::Scope::enter(&Default::default());
        let out = Path::new("/project/.jazyk/ds_main");
        let (store, gs) = fixture(out);
        let path = gs.deliverable.join("src/cart.rs");
        crate::vfs::create_dir_all(gs.deliverable.join("src")).unwrap();
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        crate::vfs::write(&path, "// req:ds_main::shop-1 hash:deadbeef\nfn cart() {}\n// req:ds_other::shop-1 hash:deadbeef\nfn other() {}\n").unwrap();
        crate::vfs::write(gs.deliverable.join("tests/cart.rs"), "fn checks_cart() {}\n").unwrap();
        mark(&store, "ent:cart", None, &serde_json::json!({"files":["src/cart.rs"],"tests":[{
            "requirement":"req:shop-1","kind":"programmatic","artifact":"tests/cart.rs","name":"checks_cart","run":"true"
        }]}), &gs).unwrap();
        let ledger = Ledger::load(out);
        let sites = &ledger.requirements["req:shop-1"].sites;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].head, "fn cart() {}");
        assert_eq!(sites[0].line, 1);
        let source = crate::vfs::read_to_string(path).unwrap();
        assert!(!source.contains("req:ds_main::"));
        assert!(source.contains("req:ds_other::shop-1"));
    }

    #[test]
    fn mark_strips_markers_into_sites() {
        let out = std::env::temp_dir().join(format!("jazyk-sites-test-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        crate::vfs::write(
            gs.deliverable.join("src/cart.rs"),
            "// req:shop-1 hash:12345678\nfn hold(i: Item) {}\n",
        )
        .ok();
        crate::vfs::write(
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
        let product = crate::vfs::read_to_string(gs.deliverable.join("src/cart.rs")).unwrap();
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
        let out = std::env::temp_dir().join(format!("jazyk-gen-contra-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
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

        crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        crate::vfs::write(gs.deliverable.join("cart.rs"), "fn hold() {}\n").ok();
        crate::vfs::write(
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
            tests: std::collections::BTreeMap::new(),
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
        let mut ledger = Ledger {
            medium: Some(Medium {
                form: "Python package".into(),
                produced: "written".into(),
                toolchain: "python3 with pytest".into(),
                artifact: String::new(),
            }),
            ..Default::default()
        };
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
        let out = std::env::temp_dir().join(format!("jazyk-gen-diverge-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        crate::vfs::create_dir_all(&out).ok();
        let ledger = Ledger {
            medium: Some(Medium {
                form: "Python package".into(),
                produced: "written".into(),
                toolchain: "python3 with pytest".into(),
                artifact: String::new(),
            }),
            ..Default::default()
        };
        ledger.save(&out).unwrap();
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        crate::vfs::write(gs.deliverable.join("cart.rs"), "fn hold() {}\n").ok();
        crate::vfs::write(
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
        let out = std::env::temp_dir().join(format!("jazyk-gen-build-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        assert!(task_package(&s, "ent:cart", &gs).unwrap()["build"].is_null());

        crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// product").ok();
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
        crate::vfs::remove_dir_all(&out).ok();
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
        let out = std::env::temp_dir().join(format!("jazyk-gen-extra-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
        for f in ["src/cart.rs", "src/cart_util.rs", "build.rs", "Cargo.toml"] {
            crate::vfs::write(gs.deliverable.join(f), "// x").ok();
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
        crate::vfs::remove_dir_all(&out).ok();
    }

    #[test]
    fn written_artifact_is_implementation_and_detects_code_changes() {
        let _scope = crate::vfs::Scope::enter(&Default::default());
        let out = Path::new("/project/.jazyk/ds_main");
        let (store, gs) = fixture(out);
        let mut ledger = Ledger::load(out);
        ledger.medium = Some(Medium {
            form: "POSIX shell script".into(),
            produced: "written".into(),
            toolchain: "sh".into(),
            artifact: "hello.sh".into(),
        });
        ledger.save(out).unwrap();
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        crate::vfs::write(gs.deliverable.join("hello.sh"), "printf 'hello\\n'\n").unwrap();
        crate::vfs::write(gs.deliverable.join("tests/hello.sh"), "# checks_hello\nsh hello.sh\n").unwrap();
        mark(&store, "ent:cart", None, &serde_json::json!({
            "files": ["hello.sh"],
            "tests": [{"requirement": "req:shop-1", "kind": "programmatic",
                "artifact": "tests/hello.sh", "name": "checks_hello", "run": "sh tests/hello.sh"}]
        }), &gs).unwrap();
        let ledger = Ledger::load(out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(row.files, vec!["hello.sh"]);
        assert_eq!(crate::verify::status_of(&store, "req:shop-1", row, &gs).0, "unverified");
        crate::vfs::write(gs.deliverable.join("hello.sh"), "printf 'changed\\n'\n").unwrap();
        assert_eq!(crate::verify::status_of(&store, "req:shop-1", row, &gs).0, "stale-code");
    }

    // Two writers at once never lose each other's change: every load, modify, and save
    // runs under the ledger lock (docs/consumers/gen.md#the-ledger).
    #[test]
    fn concurrent_ledger_updates_keep_both_changes() {
        let out = std::env::temp_dir().join(format!("jazyk-ledger-lock-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        Ledger::default().save(&out).unwrap();
        let writers: Vec<_> = ["a.css", "b.css", "c.css"]
            .into_iter()
            .map(|path| {
                let out = out.clone();
                std::thread::spawn(move || {
                    Ledger::update(&out, |ledger| {
                        std::thread::sleep(std::time::Duration::from_millis(150));
                        ledger.support.push(path.to_string());
                        Ok(())
                    })
                    .unwrap();
                })
            })
            .collect();
        for w in writers {
            w.join().unwrap();
        }
        let mut support = Ledger::load(&out).support;
        support.sort();
        assert_eq!(support, vec!["a.css", "b.css", "c.css"]);
        // Nested on one thread, the lock is reentrant.
        let _outer = Ledger::lock(&out);
        Ledger::update(&out, |_| Ok(())).unwrap();
        crate::vfs::remove_dir_all(&out).ok();
    }

    #[test]
    fn built_medium_demands_a_build_that_names_the_artifact() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-medium-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        let mut ledger = Ledger::load(&out);
        ledger.medium = Some(Medium {
            form: "Microsoft PowerPoint deck".into(),
            produced: "built".into(),
            toolchain: "python3 with python-pptx".into(),
            artifact: "jazyk.pptx".into(),
        });
        ledger.save(&out).unwrap();
        assert!(build_outputs(&ledger).contains("jazyk.pptx"));
        // The decision reaches the task package.
        let task = task_package(&s, "ent:cart", &gs).unwrap();
        assert_eq!(task["medium"]["produced"], "built");
        assert_eq!(task["medium"]["artifact"], "jazyk.pptx");

        crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
        // A marked file: the rejection must leave its markers for the retry.
        let marked = "// req:shop-1 hash:abcd1234\nfn hold() {}\n";
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), marked).ok();
        let no_build = serde_json::json!({"files": ["src/cart.rs"], "tests": []});
        let err = mark(&s, "ent:cart", None, &no_build, &gs).unwrap_err();
        assert!(err.contains("no build is recorded"), "{}", err);
        assert_eq!(crate::vfs::read_to_string(gs.deliverable.join("src/cart.rs")).unwrap(), marked);

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
        crate::vfs::remove_dir_all(&out).ok();
    }

    // Fresh obligations can develop source and tests together. Fully current
    // adopted rows stay unchanged without needing an entity generation entry.
    #[test]
    fn pending_diff_and_mark_lifecycle() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-test-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        assert_eq!(pending(&s, &gs)[0]["reason"], "incomplete-bindings");
        assert!(skip_reason(&s, &gs, "ent:cart").contains("generation work remains"));

        // Binding finds nothing: the row reads unimplemented and the entity is work.
        crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        crate::vfs::write(
            gs.deliverable.join("tests/cart.rs"),
            format!("// req:shop-1\nfn {}() {{}}", name),
        )
        .ok();
        let bound = json!({"kind": "programmatic", "artifact": "tests/cart.rs", "name": name, "run": format!("cargo test {}", name)});
        crate::bind::record(&s, "req:shop-1", &[], &bound, &json!([]), "fail", None, &gs).unwrap();
        let p = pending(&s, &gs);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["reason"], "unimplemented-bindings");
        assert_eq!(p[0]["changed"][0], "req:shop-1");

        // Binding that found the code verified: adopted, nothing to generate.
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// product").ok();
        crate::bind::record(&s, "req:shop-1", &["src/cart.rs".into()], &bound, &json!([]), "pass", None, &gs)
            .unwrap();
        assert!(pending(&s, &gs).is_empty(), "adopted code is not generation work");
        assert!(skip_reason(&s, &gs, "ent:cart").contains("no ledger entry"));
        crate::bind::record(&s, "req:shop-1", &[], &bound, &json!([]), "fail", None, &gs).unwrap();

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
        // A red verdict over the generated facts is a finding, never more generation
        // work: generation had its turn (docs/consumers/gen.md#status-is-derived-never-stored).
        crate::bind::record(&s, "req:shop-1", &[], &bound, &json!([]), "fail", None, &gs).unwrap();
        let ledger = Ledger::load(&out);
        let (st, why) = crate::verify::status_of(&s, "req:shop-1", &ledger.requirements["req:shop-1"], &gs);
        assert_eq!((st.as_str(), why.as_str()), ("failing", "criteria-failing-after-generation"));
        assert!(pending(&s, &gs).is_empty(), "generation had its turn");

        // A recorded file deleted by hand is generation work under its own name.
        crate::vfs::remove_file(gs.deliverable.join("src/cart.rs")).unwrap();
        let p = pending(&s, &gs);
        assert_eq!(p[0]["reason"], "files-gone");
        assert_eq!(p[0]["changed"][0], "src/cart.rs");
        let rec = ledger_stale_records(&s, &gs);
        let g = rec.iter().find(|c| c.detail["goal"] == "generate").unwrap();
        assert_eq!(g.detail["reason"], "files-gone");
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// product").ok();

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
        let out = std::env::temp_dir().join(format!("jazyk-prune-test-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        crate::vfs::write(
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
        ledger.save(&out).unwrap();
        crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "pub struct Cart;\n").ok();
        let manifest = serde_json::json!({
            "files": ["src/cart.rs", "tests/cart.rs"],
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

    // The generate gate is the ledger's word: no record, a stale factHash, or
    // recorded files that are not there each bounce a done claim naming the repair;
    // with run_tests required, a programmatic row never run bounces it too.
    // Mirrors docs/compiler/goals/generate.md#gate.
    #[test]
    fn the_generate_gate_reads_the_landed_record() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-gate-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (mut s, gs) = fixture(&out);
        let err = gate(&s, &gs, "ent:cart", false).unwrap_err();
        assert!(err.contains("record_generation has not landed"), "{}", err);
        crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "fn a() {}\n").unwrap();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        crate::vfs::write(gs.deliverable.join("tests/cart.rs"), format!("fn {}() {{}}\n", name)).unwrap();
        let manifest = json!({"files": ["src/cart.rs", "tests/cart.rs"], "tests": [{
            "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
            "artifact": "tests/cart.rs", "name": name, "run": "true", "files": ["src/cart.rs"]}]});
        mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
        assert_eq!(gate(&s, &gs, "ent:cart", false).unwrap(), 2);
        let err = gate(&s, &gs, "ent:cart", true).unwrap_err();
        assert!(err.contains("run_tests has not run") && err.contains("req:shop-1"), "{}", err);
        crate::verify::run_selected(&s, &gs, &[]).unwrap();
        gate(&s, &gs, "ent:cart", true).unwrap();
        crate::vfs::remove_file(gs.deliverable.join("src/cart.rs")).unwrap();
        let err = gate(&s, &gs, "ent:cart", true).unwrap_err();
        assert!(err.contains("do not exist") && err.contains("src/cart.rs"), "{}", err);
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "fn a() {}\n").unwrap();
        s.graph.entities.get_mut("ent:cart").unwrap().definition = Some("moved".into());
        let err = gate(&s, &gs, "ent:cart", false).unwrap_err();
        assert!(err.contains("facts moved"), "{}", err);
        crate::vfs::remove_dir_all(&out).ok();
    }

    // Every path a manifest names is confined before any side effect: an absolute
    // path or one climbing out with `..` is rejected naming it, and the rejected
    // record strips no marker, writes no ledger, and removes nothing.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    #[test]
    fn manifest_paths_are_confined_to_the_deliverable() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-confine-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
        // A file beside the deliverable that a climbing path could reach.
        crate::vfs::write(out.join("secret.txt"), "// req:shop-1 hash:deadbeef\nkeep\n").unwrap();
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// req:shop-1 hash:deadbeef\nfn a() {}\n").unwrap();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        crate::vfs::write(gs.deliverable.join("tests/cart.rs"), format!("fn {}() {{}}\n", name)).unwrap();
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
            assert!(!Ledger::path(&out).vexists(), "a rejected record writes no ledger");
        }
        // No side effect reached either file: the markers are still there.
        assert!(crate::vfs::read_to_string(out.join("secret.txt")).unwrap().contains("hash:"));
        assert!(crate::vfs::read_to_string(gs.deliverable.join("src/cart.rs")).unwrap().contains("hash:"));
        // The clean spellings are normalized on record.
        let ok = json!({"files": ["./src/cart.rs", "src//cart.rs"], "tests": [row]});
        mark(&s, "ent:cart", None, &ok, &gs).unwrap();
        assert_eq!(Ledger::load(&out).entities["cart"].files, vec!["src/cart.rs".to_string()]);
        assert_eq!(confine_rel("a\\b\\c.py").unwrap(), "a/b/c.py");
        assert!(confine_rel("C:/x").is_err());
        assert!(confine_rel(".").is_err());
        assert_eq!(confine_cwd("").unwrap(), ".");
        crate::vfs::remove_dir_all(&out).ok();
    }

    // The record applies the artifact gate binding applies: a row whose artifact is
    // not there, or a programmatic row whose artifact lacks the declared name, is
    // rejected naming the row and the fix. Mirrors docs/compiler/goals/generate.md#gate.
    #[test]
    fn record_generation_rejects_a_test_its_artifact_does_not_carry() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-artgate-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
        crate::vfs::write(gs.deliverable.join("cart.rs"), "fn a() {}\n").unwrap();
        crate::vfs::write(gs.deliverable.join("tests/cart.rs"), "fn something_else() {}\n").unwrap();
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
        // A row without its requirement is refused by name, never dropped in silence.
        let unnamed = json!({"files": ["cart.rs"], "tests": [{
            "kind": "programmatic", "artifact": "tests/cart.rs", "name": "req_shop_1", "run": "cargo test req_shop_1"}]});
        let err = mark(&s, "ent:cart", None, &unnamed, &gs).unwrap_err();
        assert!(err.contains("`req_shop_1` names no requirement"), "{}", err);
        let no_criteria = json!({"files": ["cart.rs"], "tests": [{
            "requirement": "req:shop-1", "kind": "llm", "artifact": "criteria/req-shop-1.md", "name": name}]});
        let err = mark(&s, "ent:cart", None, &no_criteria, &gs).unwrap_err();
        assert!(err.contains("gen/"), "{}", err);
        assert!(mark(&s, "ent:cart", None, &json!({"files": ["cart.rs"], "tests": [{
            "requirement": "req:shop-1", "kind": "oracle", "artifact": "x", "name": "y", "run": "z"}]}), &gs)
            .unwrap_err()
            .contains("`programmatic`"));
        // The criteria file under the out directory satisfies an llm row.
        crate::vfs::create_dir_all(out.join("gen/criteria")).unwrap();
        crate::vfs::write(out.join("gen/criteria/req-shop-1.md"), format!("{}\nInspect the product. PASS when satisfied. FAIL otherwise.\n", criteria_front_matter("req:shop-1", None, &s.graph.requirements["req:shop-1"].statement))).unwrap();
        mark(&s, "ent:cart", None, &no_criteria, &gs).unwrap();
        crate::vfs::remove_dir_all(&out).ok();
    }

    #[test]
    fn qualified_report_selectors_record_and_stay_bound() {
        let out = std::env::temp_dir().join(format!("jazyk-qualified-report-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (store, gs) = fixture(&out);
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        crate::vfs::write(gs.deliverable.join("cart.rs"), "fn cart() {}\n").unwrap();
        crate::vfs::write(gs.deliverable.join("tests/cart.rs"), "fn qualified_case() {}\n").unwrap();
        let mut manifest = json!({"files":["cart.rs"],"tests":[{
            "requirement":"req:shop-1","kind":"programmatic","artifact":"tests/cart.rs",
            "name":"Suite::qualified_case","run":"task:check"
        }]});
        mark(&store, "ent:cart", None, &manifest, &gs).unwrap();
        assert!(crate::bind::pending(&store, &gs).is_empty());
        manifest["tests"][0]["name"] = json!("Suite::");
        assert!(mark(&store, "ent:cart", None, &manifest, &gs).is_err());
        crate::vfs::write(gs.deliverable.join("tests/cart.rs"), "fn renamed_case() {}\n").unwrap();
        assert_eq!(crate::bind::pending(&store, &gs)[0]["reason"], "artifact-gone");
        crate::vfs::remove_dir_all(out).ok();
    }

    // A path that starts with the deliverable directory's own name restates the root
    // while no directory of that name exists under it: it lands under the root and the
    // reply says so. A deliverable that holds such a directory keeps the path.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    #[test]
    fn a_path_restating_the_deliverable_name_lands_under_the_root() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-unroot-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        crate::vfs::create_dir_all(gs.deliverable.join("src")).unwrap();
        assert_eq!(unroot(&gs.deliverable, "product/src/cart.rs").as_deref(), Some("src/cart.rs"));
        assert_eq!(unroot(&gs.deliverable, "./product/src/cart.rs").as_deref(), Some("src/cart.rs"));
        assert_eq!(unroot(&gs.deliverable, "product").as_deref(), Some(""));
        assert_eq!(unroot(&gs.deliverable, "src/cart.rs"), None);
        assert_eq!(unroot(&gs.deliverable, "products/cart.rs"), None);
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "pub struct Cart;\n").unwrap();
        crate::vfs::create_dir_all(out.join("gen/criteria")).unwrap();
        crate::vfs::write(out.join("gen/criteria/req-shop-1.md"), format!("{}\nInspect the product. PASS when satisfied. FAIL otherwise.\n", criteria_front_matter("req:shop-1", None, &s.graph.requirements["req:shop-1"].statement))).unwrap();
        let r = mark(&s, "ent:cart", None, &json!({
            "files": ["product/src/cart.rs"],
            "tests": [{"requirement": "req:shop-1", "kind": "llm", "label": "judged",
                       "artifact": "criteria/req-shop-1.md", "name": "req_shop_1", "run": "llm",
                       "files": ["product/src/cart.rs"]}],
        }), &gs).unwrap();
        assert!(r["pathNote"][0].as_str().unwrap().contains("`src/cart.rs`"), "{}", r);
        let ledger = Ledger::load(&out);
        assert_eq!(ledger.entities["cart"].files, vec!["src/cart.rs".to_string()]);
        assert_eq!(ledger.requirements["req:shop-1"].files, vec!["src/cart.rs".to_string()]);
        // A deliverable that really holds `product/` keeps the path as given.
        crate::vfs::create_dir_all(gs.deliverable.join("product")).unwrap();
        assert_eq!(unroot(&gs.deliverable, "product/src/cart.rs"), None);
        crate::vfs::remove_dir_all(&out).ok();
    }

    // Ownership is the component group's: two members of one group both write and
    // record the same page, and neither is refused; an entity of another group is.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    #[test]
    fn members_of_one_group_share_a_page_and_another_group_is_refused() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-group-own-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (mut s, gs) = fixture(&out);
        let ent = |name: &str, parent: Option<&str>| Entity {
            name: name.into(),
            parent: parent.map(String::from),
            ..Default::default()
        };
        s.graph.entities.insert("ent:org".into(), ent("Org", None));
        s.graph.entities.insert("ent:operations".into(), ent("Operations", Some("ent:org")));
        s.graph.entities.insert("ent:store-manager".into(), ent("Store Manager", Some("ent:operations")));
        s.graph.entities.insert("ent:sales".into(), ent("Sales", Some("ent:org")));
        for (rid, stmt, e) in [
            ("req:d-1", "Operations moves shipments.", "ent:operations"),
            ("req:d-2", "Operations has Store Manager as a role.", "ent:store-manager"),
            ("req:d-3", "Sales sells to wholesale accounts.", "ent:sales"),
        ] {
            s.graph.requirements.insert(
                rid.into(),
                Requirement { statement: stmt.into(), entities: vec![e.into()], ..Default::default() },
            );
        }
        crate::vfs::create_dir_all(gs.deliverable.clone()).unwrap();
        crate::vfs::create_dir_all(out.join("gen/criteria")).unwrap();
        crate::vfs::write(gs.deliverable.join("departments.md"), "# Departments\n").unwrap();
        let row = |rid: &str| {
            let f = format!("criteria/{}.md", rid.replace(':', "-"));
            crate::vfs::write(out.join("gen").join(&f), format!("{}\nInspect the product. PASS when satisfied. FAIL otherwise.\n", criteria_front_matter(rid, None, &s.graph.requirements[rid].statement))).unwrap();
            json!({"requirement": rid, "kind": "llm", "label": "judged", "artifact": f,
                   "name": rid.replace([':', '-'], "_"), "run": "llm"})
        };
        assert!(file_owner(&s, &Ledger::load(&out), "ent:sales", "departments.md").is_none());
        mark(&s, "ent:operations", None, &json!({"files": ["departments.md"], "tests": [row("req:d-1")]}), &gs)
            .unwrap();
        // The same group: shared, never refused.
        mark(&s, "ent:store-manager", None, &json!({"files": ["departments.md"], "tests": [row("req:d-2")]}), &gs)
            .unwrap();
        let ledger = Ledger::load(&out);
        assert_eq!(ledger.entities["operations"].files, vec!["departments.md".to_string()]);
        assert_eq!(ledger.entities["store-manager"].files, vec!["departments.md".to_string()]);
        assert!(file_owner(&s, &ledger, "ent:store-manager", "departments.md").is_none());
        assert!(file_owner(&s, &ledger, "ent:operations", "departments.md").is_none());
        // Another group: refused, the owner named.
        assert_eq!(file_owner(&s, &ledger, "ent:sales", "departments.md").as_deref(), Some("operations"));
        let err = mark(&s, "ent:sales", None, &json!({"files": ["departments.md"], "tests": [row("req:d-3")]}), &gs)
            .unwrap_err();
        assert!(err.contains("`departments.md` belongs to entity `operations`"), "{}", err);
        assert!(err.contains("component group"), "{}", err);
        // The system generates alone: the group's page is not its own either, and a
        // requirement target (a bind goal) owns nothing.
        assert!(file_owner(&s, &ledger, "ent:org", "departments.md").is_some());
        assert!(file_owner(&s, &ledger, "req:d-1", "departments.md").is_some());
        assert_eq!(shared_with(&s, "ent:operations"), vec!["ent:store-manager".to_string()]);
        assert!(shared_with(&s, "ent:org").is_empty());
        crate::vfs::remove_dir_all(&out).ok();
    }

    #[test]
    fn scoped_worker_explicitly_promotes_and_extends_a_shared_written_page() {
        let _vfs = crate::vfs::Scope::enter(&BTreeMap::new());
        let root = Path::new("/project");
        crate::vfs::create_dir_all(root.join(".jazyk")).unwrap();
        crate::vfs::write(root.join(crate::document_sets::CONFIG), "version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Site'\ndocs = ['docs/**']\nsource = ['deliverable/**']\n").unwrap();
        let project = crate::document_sets::Workspace::load(root).unwrap().resolve(Some("ds_main")).unwrap();
        let gs = GenSettings::resolve(&project);
        let mut store = Store { out: project.out.clone(), ..Default::default() };
        for (id, name, rid, statement) in [
            ("ent:footer", "Footer", "req:footer", "The page displays the footer."),
            ("ent:top-menu", "Top Menu", "req:top-menu", "The page displays the menu."),
        ] {
            store.graph.entities.insert(id.into(), Entity { name: name.into(), ..Default::default() });
            store.graph.requirements.insert(rid.into(), Requirement {
                statement: statement.into(), entities: vec![id.into()], ..Default::default()
            });
            store.graph.criteria.insert(format!("crit:{}", id.trim_start_matches("ent:")), Criterion {
                requirement: rid.into(), statement: statement.into(), ..Default::default()
            });
        }
        store.save();
        let page = "deliverable/page.html";
        let exclusive = "deliverable/footer.css";
        let artifact = "deliverable/tests/site.py";
        crate::vfs::create_dir_all(root.join("deliverable/tests")).unwrap();
        crate::vfs::write(root.join(page), "<html><body>\n<!-- req:footer hash:12345678 -->\n<footer>Existing footer</footer>\n</body></html>\n").unwrap();
        crate::vfs::write(root.join(exclusive), "footer { color: navy; }\n").unwrap();
        crate::vfs::write(root.join(artifact), "def checks_footer():\n    pass\ndef checks_menu():\n    pass\n").unwrap();
        let mut ledger = Ledger::load(&store.out);
        ledger.medium = Some(Medium {
            form: "Static HTML page".into(), produced: "written".into(),
            toolchain: "HTML".into(), artifact: page.into(),
        });
        ledger.save(&store.out).unwrap();
        let footer_test = json!({"requirement":"req:footer","criterion":"crit:footer","kind":"programmatic",
            "artifact":artifact,"name":"checks_footer","run":"task:site","files":[page]});
        let mut footer_manifest = json!({"files":[page,exclusive],"tests":[footer_test]});
        mark(&store, "ent:footer", None, &footer_manifest, &gs).unwrap();
        let original = crate::vfs::read_to_string(root.join(page)).unwrap();
        let mut ledger = Ledger::load(&store.out);
        let footer = ledger.requirements.get_mut("req:footer").unwrap();
        footer.verdict = "pass".into();
        footer.evidence = Some("Prior fixture judgment of these exact source bytes.".into());
        for test in footer.tests.values_mut() {
            test.verdict = "pass".into();
            test.evidence = footer.evidence.clone();
        }
        ledger.save(&store.out).unwrap();
        let prior_row = serde_json::to_value(&ledger.requirements["req:footer"]).unwrap();
        assert_eq!(crate::verify::status_of(&store, "req:footer", &ledger.requirements["req:footer"], &gs).0, "verified");
        assert!(shared_with(&store, "ent:top-menu").is_empty());

        // This is the same target fence as a Top Menu-only in-process worker.
        let server = crate::mcp::McpServer::with_bridge(project.clone(), store.out.clone(),
            vec!["generate".into()], true, crate::mcp::BridgeFlags {
                ephemeral: true, serve_files: true,
                only: Some("g:generate:ent:top-menu".into()),
                build_token: Some("shared-page-fixture".into()), ..Default::default()
            });
        let call = |name: &str, args: Value| server.handle("tools/call", &json!({"name":name,"arguments":args})).unwrap();
        let successful = |reply: Value| -> Value {
            assert!(!reply["isError"].as_bool().unwrap_or(false), "{reply}");
            serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap()
        };
        let refused = call("write_text_file", json!({"path":page,"content":"must not replace footer"}));
        assert!(refused.to_string().contains("file-owned"), "{refused}");
        assert_eq!(crate::vfs::read_to_string(root.join(page)).unwrap(), original);

        // Reopen the recorded owner, preserve its bytes and complete manifest,
        // and explicitly add the real shared composition page to support.
        let owner = successful(call("begin_generation", json!({"entity":"ent:footer"})));
        footer_manifest["supportFiles"] = json!([page]);
        successful(call("record_generation", json!({"entity":"ent:footer",
            "factHash":owner["factHash"],"manifest":footer_manifest})));
        let promoted = Ledger::load(&store.out);
        assert_eq!(promoted.support, [page]);
        assert_eq!(crate::vfs::read_to_string(root.join(page)).unwrap(), original);
        assert_eq!(serde_json::to_value(&promoted.requirements["req:footer"]).unwrap(), prior_row);
        assert!(file_owner(&store, &promoted, "ent:top-menu", page).is_none());

        let package = successful(call("begin_generation", json!({"entity":"ent:top-menu"})));
        assert_eq!(package["sharedWith"], json!([]));
        assert_eq!(package["supportFiles"], json!([page]));
        let next = original.replace("<html><body>\n", "<html><body>\n<!-- req:top-menu hash:12345678 -->\n<nav>New menu</nav>\n");
        successful(call("write_text_file", json!({"path":page,"content":next})));
        let exclusive_refusal = call("write_text_file", json!({"path":exclusive,"content":"must not replace exclusive module"}));
        assert!(exclusive_refusal.to_string().contains("file-owned"), "{exclusive_refusal}");
        assert_eq!(crate::vfs::read_to_string(root.join(exclusive)).unwrap(), "footer { color: navy; }\n");
        let outside = call("write_text_file", json!({"path":"private/page.html","content":"outside Source"}));
        assert!(outside.to_string().contains("source-selection"), "{outside}");
        let bad_promotion = call("record_generation", json!({"entity":"ent:top-menu",
            "factHash":package["factHash"],"manifest":{"files":[page],"supportFiles":["private/page.html"]}}));
        assert!(bad_promotion.to_string().contains("source selection"), "{bad_promotion}");
        assert_eq!(Ledger::load(&store.out).support, [page]);
        successful(call("record_generation", json!({"entity":"ent:top-menu",
            "factHash":package["factHash"],"manifest":{"files":[page],"supportFiles":[page],"tests":[{
                "requirement":"req:top-menu","criterion":"crit:top-menu","kind":"programmatic","artifact":artifact,
                "name":"checks_menu","run":"task:site","files":[page]
            }]}})));

        let ledger = Ledger::load(&store.out);
        let content = crate::vfs::read_to_string(root.join(page)).unwrap();
        assert!(content.contains("<nav>New menu</nav>"));
        assert!(content.contains("<footer>Existing footer</footer>"));
        assert!(!content.contains("hash:"));
        assert_eq!(ledger.entities["footer"].files, [page, exclusive]);
        assert_eq!(ledger.entities["top-menu"].files, [page]);
        let menu = &ledger.requirements["req:top-menu"];
        assert_eq!(menu.files, [page]);
        assert_eq!(menu.sites[0].head, "<nav>New menu</nav>");
        assert_eq!(menu.hashes.files, hash_implementing(&store.out, &gs, &menu.files));
        assert_eq!(crate::verify::status_of(&store, "req:top-menu", menu, &gs).0, "unverified");
        let footer = &ledger.requirements["req:footer"];
        assert_eq!(footer.sites[0].head, "<footer>Existing footer</footer>");
        assert!(locate_head(&content, &footer.sites[0].head, footer.sites[0].line).is_some());
        assert_eq!(crate::verify::status_of(&store, "req:footer", footer, &gs).0, "stale-code");
        assert!(file_owner(&store, &ledger, "ent:top-menu", exclusive).is_some());
        assert!(!root.join("private/page.html").vexists());
    }

    // A record names the files that realize the entity's requirements: files that
    // hold nothing but tests are refused naming the requirements nothing carries,
    // while an entity whose requirement another entity's files carry records
    // files: [] and its row keeps those files.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    #[test]
    fn a_record_with_no_implementing_file_is_refused_unless_carried() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-no-impl-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (mut s, gs) = fixture(&out);
        s.graph.entities.insert("ent:buyer".into(), Entity { name: "Buyer".into(), ..Default::default() });
        s.graph.requirements.get_mut("req:shop-1").unwrap().entities =
            vec!["ent:cart".into(), "ent:buyer".into()];
        crate::vfs::create_dir_all(gs.deliverable.join("src")).unwrap();
        crate::vfs::create_dir_all(gs.deliverable.join("tests/criteria")).unwrap();
        crate::vfs::write(gs.deliverable.join("tests/criteria/req-shop-1.md"), format!("{}\nInspect the product. PASS when satisfied. FAIL otherwise.\n", criteria_front_matter("req:shop-1", None, &s.graph.requirements["req:shop-1"].statement))).unwrap();
        let row = json!({"requirement": "req:shop-1", "kind": "llm", "label": "judged",
                         "artifact": "tests/criteria/req-shop-1.md", "name": "req_shop_1", "run": "llm"});
        // Nothing but the criteria file: refused, the requirement named.
        for files in [json!([]), json!(["tests/criteria/req-shop-1.md"])] {
            let err = mark(&s, "ent:cart", None, &json!({"files": files, "tests": [row.clone()]}), &gs)
                .unwrap_err();
            assert!(err.contains("req:shop-1"), "{}", err);
            assert!(err.contains("not implementation"), "{}", err);
        }
        assert!(Ledger::load(&out).entities.is_empty(), "a refusal records nothing");
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "pub struct Cart;\n").unwrap();
        mark(&s, "ent:cart", None, &json!({"files": ["src/cart.rs", "tests/criteria/req-shop-1.md"], "tests": [row.clone()]}), &gs)
            .unwrap();
        assert_eq!(
            Ledger::load(&out).requirements["req:shop-1"].files,
            vec!["src/cart.rs".to_string()],
            "a test artifact is never a row's implementing file"
        );
        // The buyer's one requirement is carried by the cart's file: files: [] stands,
        // and listing the shared row unchanged keeps the file that carries it.
        mark(&s, "ent:buyer", None, &json!({"files": [], "tests": [row]}), &gs).unwrap();
        let ledger = Ledger::load(&out);
        assert!(ledger.entities["buyer"].files.is_empty());
        assert_eq!(ledger.requirements["req:shop-1"].files, vec!["src/cart.rs".to_string()]);
        assert!(gate(&s, &gs, "ent:buyer", false).is_ok());
        crate::vfs::remove_dir_all(&out).ok();
    }

    // A regeneration replaces the file set through the record itself, whichever
    // worker records: what the previous record listed and this manifest omits is
    // removed (snapshotted first), while a file another entity records, a support
    // file, or a row's test artifact stays.
    // Mirrors docs/consumers/gen.md#incremental-regeneration.
    #[test]
    fn a_record_removes_the_files_its_manifest_omits() {
        let out = std::env::temp_dir().join(format!("jazyk-gen-remove-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (mut s, gs) = fixture(&out);
        // Shelf is part of Cart, one component group under the Shop system, so the
        // two share their files.
        s.graph.entities.insert("ent:shop".into(), Entity { name: "Shop".into(), ..Default::default() });
        s.graph.entities.get_mut("ent:cart").unwrap().parent = Some("ent:shop".into());
        s.graph.entities.insert(
            "ent:shelf".into(),
            Entity { name: "Shelf".into(), parent: Some("ent:cart".into()), ..Default::default() },
        );
        crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
        for f in ["src/cart.rs", "src/cart_old.rs", "src/shared.rs", "Cargo.toml", "src/cart_v2.rs"] {
            crate::vfs::write(gs.deliverable.join(f), format!("// {}\n", f)).unwrap();
        }
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        crate::vfs::write(gs.deliverable.join("tests/cart.rs"), format!("fn {}() {{}}\n", name)).unwrap();
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
        assert!(!gs.deliverable.join("src/cart_old.rs").vexists());
        assert!(!gs.deliverable.join("src/cart.rs").vexists());
        assert!(gs.deliverable.join("src/shared.rs").vexists(), "another entity records it");
        assert!(gs.deliverable.join("Cargo.toml").vexists(), "support files are the deliverable's");
        assert!(gs.deliverable.join("tests/cart.rs").vexists());
        assert!(out.join("deliverable-baseline/src/cart_old.rs").vexists(), "snapshotted before removal");
        crate::vfs::remove_dir_all(&out).ok();
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
        let out = std::env::temp_dir().join(format!("jazyk-facthash-test-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
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
        crate::vfs::remove_dir_all(&out).ok();
    }

    // One invented-choice diagnostic per manifest entry, graded by the scope of the
    // invention, with the proposal prompt and the measure in the message.
    // Mirrors docs/consumers/gen.md#invented-choices.
    #[test]
    fn invented_choices_stage_one_graded_diagnostic_per_scope() {
        let out = std::env::temp_dir().join(format!("jazyk-choice-test-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
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
        crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "fn hold() {}\n").ok();
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
        crate::vfs::remove_dir_all(&out).ok();
    }

    // The unattached remainder on a two-file fixture: one owned file no row names,
    // and the significant lines outside every site's run.
    // Mirrors docs/consumers/gen.md#the-unattached-remainder.
    #[test]
    fn unattached_remainder_counts_unnamed_files_and_lines_outside_site_runs() {
        let out = std::env::temp_dir().join(format!("jazyk-unattached-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
        // Two significant lines sit before the first site's run.
        crate::vfs::write(
            gs.deliverable.join("src/cart.rs"),
            "// header comment\nuse std::fmt;\nfn helper() {}\n// req:shop-1 hash:12345678\nfn hold(i: Item) {}\nfn hold_more() {}\n",
        )
        .ok();
        // A second owned file no requirement row names: unattached whole.
        crate::vfs::write(
            gs.deliverable.join("src/extra.rs"),
            "fn extra() {}\n\n// note\nfn more() {}\n",
        )
        .ok();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        crate::vfs::write(
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
        crate::vfs::remove_dir_all(&out).ok();
    }

    // An unbound requirement yields a ledger-stale change record naming its goal,
    // so the board derives bind (and generate for the entity) from it.
    // Mirrors docs/compiler/goals/bind.md#created-when.
    #[test]
    fn an_unbound_requirement_yields_a_ledger_stale_record() {
        let out = std::env::temp_dir().join(format!("jazyk-stale-rec-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
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
        let generation = records.iter().find(|record| record.detail["goal"] == "generate").unwrap();
        assert_eq!(generation.subject, "ent:cart");
        assert_eq!(generation.detail["reason"], "incomplete-bindings");
        // Bound and unimplemented: the entity is generation work under that reason.
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        crate::vfs::write(gs.deliverable.join("tests/cart.rs"), format!("fn {}() {{}}", name)).ok();
        let bound = json!({"kind": "programmatic", "artifact": "tests/cart.rs", "name": name, "run": format!("cargo test {}", name)});
        crate::bind::record(&s, "req:shop-1", &[], &bound, &json!([]), "fail", None, &gs).unwrap();
        let records = ledger_stale_records(&s, &gs);
        let g = records
            .iter()
            .find(|r| r.detail["goal"] == "generate")
            .expect("a generate record");
        assert_eq!(g.subject, "ent:cart");
        assert_eq!(g.detail["reason"], "unimplemented");
        assert_eq!(g.detail["changed"][0], "req:shop-1");
        crate::vfs::remove_dir_all(&out).ok();
    }
}

// ---- the built-in worker ----
// Moved out of the CLI so the GUI job runner drives the same loop. The CLI wrapper
// renders the worker events on the historical output format; the GUI streams them.

// Run generation over the given entities (all entities with requirements when empty),
// leaf-first over the relationship graph, skipping entities whose facts are unchanged
// unless forced. Returns {regenerated, skipped, failures}.
#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub async fn run_all(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
    gs: &GenSettings,
    entities: &[String],
    force: bool,
    trace: &crate::session::Trace,
) -> Result<Value, String> {
    run_selected(store, runner, gs, entities, force, trace, true).await
}

#[derive(Default, Serialize, Deserialize)]
struct GenCommand {
    groups: Option<Vec<(String, Vec<String>)>>,
    pending: std::collections::BTreeSet<String>,
    cursor: usize,
    active: Option<GenGroup>,
    medium_decided: bool,
    regenerated: u64,
    skipped: u64,
    failures: u64,
    streak: usize,
    stopped: Option<String>,
    #[serde(default)]
    first_error: Option<String>,
    result: Option<Value>,
}

#[derive(Serialize, Deserialize)]
struct GenGroup {
    root: String,
    ready: Vec<String>,
    pipeline_cursor: usize,
    pipeline_task: Option<Value>,
    pipeline_results: Vec<(String, Result<usize, String>)>,
    #[serde(default)]
    pipeline_failed: Option<String>,
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub(crate) async fn run_selected(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
    gs: &GenSettings,
    entities: &[String],
    force: bool,
    trace: &crate::session::Trace,
    build_product: bool,
) -> Result<Value, String> {
    use crate::session::TraceEvent;
    let admitted = progress::admitted_settings("generate", &store.out,
        json!({"entities":entities,"force":force,"buildProduct":build_product}),gs).await?;
    let signature = json!({"settings": admitted, "entities": entities, "force": force, "buildProduct": build_product});
    let key = progress::command_key("generate", &store.out, &signature);
    let mut command = progress::Progress::load(key, signature, GenCommand::default)?;
    if let Some(result) = &command.data.result {
        return Ok(result.clone());
    }
    if command.data.groups.is_none() {
        let targets: Vec<String> = if entities.is_empty() {
            store
                .graph
                .entities
                .keys()
                .filter(|id| !store.requirements_referencing(id).is_empty())
                .cloned()
                .collect()
        } else {
            let mut targets = Vec::new();
            for entity in entities {
                let id = store.resolve_id(entity).to_string();
                if !store.graph.entities.contains_key(&id) {
                    return Err(format!("unknown entity `{entity}`"));
                }
                targets.push(id);
            }
            targets
        };
        if targets.is_empty() {
            return Err("no entities with requirements; run `jazyk compile` first".into());
        }
        command.data.groups = Some(groups_in_order(store, &generation_order(store, &targets)));
        command.data.pending = pending(store, gs)
            .iter()
            .filter_map(|item| item["entity"].as_str().map(String::from))
            .collect();
        command.save().await?;
    }
    // Medium selection is a command step. An empty ledger on resume does not
    // start this decision again after a later group has already begun.
    if !command.data.medium_decided {
        // The decision is a model call, made outside the ledger lock; it lands on a
        // fresh load unless another writer decided first.
        if Ledger::load(&store.out).medium.is_none() {
            let medium = decide_medium(store, runner, &gs.deliverable).await?;
            trace.line("gen medium", &medium.line());
            Ledger::update(&store.out, |ledger| {
                ledger.medium.get_or_insert(medium);
                Ok(())
            })?;
        }
        command.data.medium_decided = true;
        command.save().await?;
    }
    crate::vfs::create_dir_all(&gs.deliverable).ok();
    while command.data.cursor < command.data.groups.as_ref().unwrap().len() {
        let current_settings = gs.current_for_store(store);
        let gs = &current_settings;
        if command.data.stopped.is_some() {
            break;
        }
        if trace.is_cancelled() {
            break;
        }
        if crate::session_recovery::suspension_requested() {
            command.save().await?;
            return Err("suspended".into());
        }
        if command.data.active.is_none() {
            let (root, members) =
                command.data.groups.as_ref().unwrap()[command.data.cursor].clone();
            let mut ready = Vec::new();
            for id in members {
                if !force && !command.data.pending.contains(&id) {
                    command.data.skipped += 1;
                    trace.event(TraceEvent::GenEntitySkipped {
                        entity: id.clone(),
                        reason: skip_reason(store, gs, &id),
                    });
                } else {
                    ready.push(id);
                }
            }
            if ready.is_empty() {
                command.data.cursor += 1;
                command.save().await?;
                continue;
            }
            command.data.active = Some(GenGroup {
                root,
                ready,
                pipeline_cursor: 0,
                pipeline_task: None,
                pipeline_results: Vec::new(),
                pipeline_failed: None,
            });
            command.save().await?;
        }
        let active = command.data.active.as_ref().unwrap();
        let (root, ready) = (active.root.clone(), active.ready.clone());
        for id in &ready {
            trace.event(TraceEvent::GenEntityStart { entity: id.clone() });
        }
        let outcome = if gs.worker == "pipeline" {
            while command.data.active.as_ref().unwrap().pipeline_cursor < ready.len()
                && command.data.active.as_ref().unwrap().pipeline_failed.is_none()
                && !command.data.active.as_ref().unwrap().pipeline_results.iter().any(|(_, result)| result.is_err()) {
                let cursor = command.data.active.as_ref().unwrap().pipeline_cursor;
                let id = &ready[cursor];
                if trace.is_cancelled() {
                    break;
                }
                if crate::session_recovery::suspension_requested() {
                    command.save().await?;
                    return Err("suspended".into());
                }
                if command
                    .data
                    .active
                    .as_ref()
                    .unwrap()
                    .pipeline_task
                    .is_none()
                {
                    match task_package(store, id, gs) {
                        Ok(task) => {
                            command.data.active.as_mut().unwrap().pipeline_task = Some(task);
                            command.save().await?;
                        }
                        Err(error) => {
                            let active = command.data.active.as_mut().unwrap();
                            active.pipeline_results.push((id.clone(), Err(error)));
                            active.pipeline_cursor += 1;
                            command.save().await?;
                            continue;
                        }
                    }
                }
                let task = command
                    .data
                    .active
                    .as_ref()
                    .unwrap()
                    .pipeline_task
                    .as_ref()
                    .unwrap()
                    .clone();
                let outcome = pipeline::run_report(store, runner, gs, id, &task).await?;
                let result = outcome.result;
                if let Err(reason) = &result {
                    if crate::session_recovery::is_interruption(reason) {
                        return Err(reason.clone());
                    }
                }
                let active = command.data.active.as_mut().unwrap();
                active.pipeline_failed = outcome.failed;
                active.pipeline_results.push((id.clone(), result));
                active.pipeline_cursor += 1;
                active.pipeline_task = None;
                command.save().await?;
            }
            GenSessionOutcome {
                results: command
                    .data
                    .active
                    .as_ref()
                    .unwrap()
                    .pipeline_results
                    .clone(),
                tokens: 0,
                failed: command.data.active.as_ref().unwrap().pipeline_failed.clone(),
            }
        } else {
            gen_session(store, runner, &admitted, &root, &ready, trace).await
        };
        if let Some(reason) = outcome
            .failed
            .as_ref()
            .filter(|reason| crate::session_recovery::is_interruption(reason))
        {
            return Err(reason.clone());
        }
        let (next, _) = crate::reconcile::breaker_step(
            command.data.streak,
            outcome.tokens,
            outcome.failed.as_deref(),
        );
        command.data.streak = next;
        let incomplete: Vec<_> = ready.iter().filter_map(|id| {
            match outcome.results.iter().find(|(target, _)| target == id).map(|(_, result)| result) {
                Some(Ok(_)) => None,
                Some(Err(error)) => Some((id.clone(), error.clone())),
                None => Some((id.clone(), outcome.failed.clone().unwrap_or_else(||
                    "The group ended before completing this target".into()))),
            }
        }).collect();
        for (id, result) in outcome.results {
            match result {
                Ok(files) => {
                    trace.event(TraceEvent::GenEntityDone { entity: id, files });
                    command.data.regenerated += 1;
                }
                Err(error) => {
                    if outcome.failed.is_none() {
                        command.data.first_error.get_or_insert_with(||
                            format!("{} was not completed: {error}", store.graph.entities.get(&id)
                                .map(|entity| entity.name.as_str()).filter(|name| !name.is_empty())
                                .unwrap_or("This part of the project")));
                    }
                    trace.event(TraceEvent::GenEntityFailed {
                        entity: id,
                        stage: "generate".into(),
                        error,
                    });
                    command.data.failures += 1;
                }
            }
        }
        command.data.active = None;
        command.data.cursor += 1;
        crate::session_recovery::remove_frame(&crate::session_recovery::batch_key(
            &store.out,
            &format!("g:generate:{root}"),
        ));
        if !incomplete.is_empty() {
            let name = |id: &str| store.graph.entities.get(id).map(|entity| entity.name.as_str())
                .filter(|name| !name.is_empty()).unwrap_or(id).to_owned();
            let owed = incomplete.iter().map(|(id, _)| name(id)).collect::<Vec<_>>().join(", ");
            let reason = format!("Generation group {} is incomplete. Still owed: {owed}. {}",
                name(&root), command.data.first_error.as_deref().unwrap_or(&incomplete[0].1));
            command.data.first_error = Some(reason.clone());
            command.data.stopped = Some(outcome.failed.unwrap_or(reason));
        } else if outcome.failed.is_some() {
            command.data.stopped = outcome.failed;
        }
        command.save().await?;
        if command.data.stopped.is_some() {
            break;
        }
    }
    let build = if !build_product || trace.is_cancelled() {
        json!({"state":"not-selected"})
    } else if command.data.failures > 0 || command.data.stopped.is_some() {
        json!({"state":"not-run", "reason":"generation is incomplete"})
    } else if crate::vfs::is_scoped() {
        json!({"state":"pending-ci", "pendingCi": Ledger::load(&store.out).build.is_some(), "reason":"build commands run through delivery CI"})
    } else {
        match run_build(&store.out, gs, trace, "gen") {
            Ok(()) => {
                let produced = Ledger::load(&store.out)
                    .build
                    .map(|build| build.produces.join(", "))
                    .filter(|paths| !paths.is_empty());
                if let Some(paths) = &produced {
                    trace.line("gen", &format!("built {paths}"));
                }
                json!({"ok": true, "produced": produced})
            }
            Err(error) => {
                if crate::session_recovery::is_interruption(&error) {
                    return Err(error);
                }
                trace.line("gen", &error);
                json!({"ok": false, "error": error})
            }
        }
    };
    let mut result = json!({"regenerated":command.data.regenerated,"skipped":command.data.skipped,
        "failures":command.data.failures,"stopped":command.data.stopped,"build":build});
    if let Some(error) = &command.data.first_error {
        result["error"] = json!(error);
    }
    command.data.result = Some(result.clone());
    command.save().await?;
    Ok(result)
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
#[derive(Serialize, Deserialize)]
struct GenSessionState {
    session: progress::BatchProgress,
    before: Vec<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before_sequences: Option<Vec<Option<u64>>>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    completed: std::collections::BTreeSet<usize>,
    #[serde(default, skip_serializing_if = "record_sequence_is_zero")]
    prior_tokens: u64,
    result: Option<GenSessionOutcome>,
}

impl GenSessionState {
    fn recorded(&self, index: usize, row: Option<&EntityGen>) -> bool {
        if self.completed.contains(&index) { return true; }
        match &self.before_sequences {
            Some(sequences) => sequences.get(index).is_some_and(|before| {
                row.is_some_and(|row| row.record_sequence > before.unwrap_or(0))
            }),
            None => row.and_then(|row| row.recorded_at.as_ref()) != self.before[index].as_ref(),
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
async fn gen_session(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
    gs: &GenSettings,
    root: &str,
    ids: &[String],
    trace: &crate::session::Trace,
) -> GenSessionOutcome {
    let error = |reason: String| GenSessionOutcome {
        results: Vec::new(),
        tokens: 0,
        failed: Some(reason),
    };
    // The admitted group keeps the pre-input fingerprint identity. Referenced
    // bytes can change while a run is suspended, or become dependencies after a
    // release. They belong to its current gate, never its recovery lookup key.
    let signature = json!({"settings":gs,"root":root,"ids":ids,
        "facts":ids.iter().map(|id| fact_hash_base(store,id,&reqs_of_sorted(store,id))).collect::<Vec<_>>()});
    let key = progress::command_key("generation-session", &store.out, &signature);
    let mut saved = match progress::Progress::load(key, signature, || {
        let batch = crate::runner::BatchRun {
            id: format!("g:generate:{root}"),
            executor: None,
            goals: ids
                .iter()
                .map(|id| crate::model::Goal {
                    id: format!("g:generate:{id}"),
                    kind: "generate".into(),
                    class: "compile".into(),
                    mandatory: true,
                    target: id.clone(),
                    unit: "entity".into(),
                    change: json!({"goal":"generate"}),
                    ..Default::default()
                })
                .collect(),
        };
        let ledger = Ledger::load(&store.out);
        GenSessionState {
            session: progress::BatchProgress::new(batch, &store.out),
            before: ids
                .iter()
                .map(|id| {
                    ledger
                        .entities
                        .get(&slug_of(id))
                        .and_then(|row| row.recorded_at.clone())
                })
                .collect(),
            before_sequences: Some(ids.iter().map(|id| ledger.entities.get(&slug_of(id))
                .map(|row| row.record_sequence)).collect()),
            completed: Default::default(),
            prior_tokens: 0,
            result: None,
        }
    }) {
        Ok(saved) => saved,
        Err(reason) => return error(reason),
    };
    if let Some(result) = &saved.data.result {
        return result.clone();
    }
    if let Err(reason) = saved.save().await {
        return error(reason);
    }
    loop {
        let report = match saved.data.session.report.clone() {
            Some(report) => report,
            None => match saved.data.session.committed_report(&store.out) {
                Some(report) => report,
                None => runner.run_item(&saved.data.session.batch, trace).await,
            },
        };
        if let Some(reason) = progress::interruption(&report) {
            return error(reason);
        }
        saved.data.session.report = Some(report.clone());
        if let Err(reason) = saved.save().await {
            return error(reason);
        }
        let after = Ledger::load(&store.out);
        let current_settings = gs.current_for_store(store);
        let results = ids
            .iter()
            .zip(&saved.data.before)
            .enumerate()
            .map(|(index, (id, _))| {
                let result = if !saved.data.recorded(index, after.entities.get(&slug_of(id))) {
                    Err(report.failed.clone().or_else(|| report.stopped.clone()).unwrap_or_else(||
                        "The agent ended its session before recording completion".into()))
                } else {
                    gate(store, &current_settings, id, false)
                        .map_err(|error| report.failed.clone().unwrap_or(error))
                };
                (id.clone(), result)
            })
            .collect::<Vec<_>>();
        // A pre-input-fingerprint worker may have committed just before suspension.
        // Its receipt survives, but its old base hash cannot prove the supplied input
        // was used. Continue only those newly stale members in a fresh current batch.
        // Save the transition before admitting any replacement tool/model operation.
        if saved.data.before_sequences.is_none() && report.failed.is_none() && report.stopped.is_none() {
            let failed: Vec<usize> = results.iter().enumerate()
                .filter_map(|(index,(_, result))| result.is_err().then_some(index)).collect();
            let refresh: Vec<usize> = failed.iter().copied().filter(|index| {
                let id = &ids[*index];
                let rids = reqs_of_sorted(store,id);
                let base = fact_hash_base(store,id,&rids);
                after.entities.get(&slug_of(id)).is_some_and(|row| row.fact_hash == base)
                    && assets::referenced_assets(store,id,&rids).iter().any(|asset| asset.is_input())
                    && fact_hash(store,id) != base
            }).collect();
            if !refresh.is_empty() && refresh.len() == failed.len() {
                saved.data.completed.extend(results.iter().enumerate()
                    .filter_map(|(index,(_,result))| result.is_ok().then_some(index)));
                let targets: std::collections::BTreeSet<&str> = refresh.iter().map(|index| ids[*index].as_str()).collect();
                let mut batch = saved.data.session.batch.clone();
                batch.goals.retain(|goal| targets.contains(goal.target.as_str()));
                saved.data.before = ids.iter().map(|id| after.entities.get(&slug_of(id))
                    .and_then(|row| row.recorded_at.clone())).collect();
                saved.data.before_sequences = Some(ids.iter().map(|id| after.entities.get(&slug_of(id))
                    .map(|row| row.record_sequence)).collect());
                saved.data.prior_tokens = saved.data.prior_tokens.saturating_add(report.tokens);
                saved.data.session = progress::BatchProgress::new(batch,&store.out);
                crate::session_recovery::remove_frame(&crate::session_recovery::batch_key(
                    &store.out,&saved.data.session.batch.id));
                if let Err(reason) = saved.save().await { return error(reason); }
                continue;
            }
        }
        let failed = if results.iter().any(|(_, result)| result.is_err()) {
            report.failed
        } else { None };
        let outcome = GenSessionOutcome {
            results,
            tokens: saved.data.prior_tokens.saturating_add(report.tokens),
            failed,
        };
        saved.data.result = Some(outcome.clone());
        if let Err(reason) = saved.save().await {
            return error(reason);
        }
        return outcome;
    }
}

// What one generate session came to: the per-entity results, and what the session
// spent and how it failed, for the run's dead-endpoint breaker.
#[derive(Clone, Serialize, Deserialize)]
struct GenSessionOutcome {
    results: Vec<(String, Result<usize, String>)>,
    tokens: u64,
    failed: Option<String>,
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
    match crate::vfs::read(gs.deliverable.join(rel)) {
        Ok(bytes) => {
            if let Some(p) = dst.parent() {
                crate::vfs::create_dir_all(p).ok();
            }
            crate::vfs::write(&dst, bytes).ok();
        }
        Err(_) => {
            crate::vfs::remove_file(&dst).ok();
        }
    }
}

fn snapshot_pipeline_baseline(out: &Path, gs: &GenSettings, rel: &str,
    seen: &mut std::collections::HashSet<String>, key: &str) -> Result<(), String> {
    if pipeline::first_baseline(key, rel)? { snapshot_baseline(out, gs, rel, seen); }
    else { seen.insert(rel.into()); }
    Ok(())
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

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub async fn gen_one(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
    gs: &GenSettings,
    id: &str,
    task: &serde_json::Value,
) -> Result<usize, String> {
    pipeline::run(store, runner, gs, id, task).await
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
async fn gen_one_inner(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
    gs: &GenSettings,
    id: &str,
    task: &serde_json::Value,
    ledger_snapshot: &Ledger,
    baseline_key: &str,
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
        "The deliverable: {}\n{}\nEntity {} ({})\nContext:\n{}\nChanged since last generation: {}\nWhat other entities' tasks already wrote, by entity: their `files` and the statements those files `holds`. Each file belongs to its entity's component group; never write to a file of an entity outside yours{}. Reference them instead: when your part composes theirs, read or import those paths, and let what they hold tell you what is in them: {}\nRecorded run commands (the established toolchain; reuse it): {}\nRecorded build for this deliverable: {}\n",
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
        match shared_with(store, id) {
            v if v.is_empty() => String::new(),
            v => format!(" (your group, whose files you may extend: {})", v.join(", ")),
        },
        serde_json::to_string(&task["generatedFiles"]).unwrap_or_default(),
        if run_commands.is_empty() { "(none yet; this task establishes them)" } else { &run_commands },
        match &task["build"] {
            serde_json::Value::Null => "(none yet)".to_string(),
            b => b.to_string(),
        },
    );
    // File ownership: a path recorded for an entity outside this one's component
    // group is off limits. One corrective retry, then the task fails.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    let ledger = ledger_snapshot;
    let owner_of = |path: &str| -> Option<String> { file_owner(store, &ledger, id, path) };
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
            ).await
            .map_err(|e| pipeline::step_error(format!("product part {}/{}", k + 1, parts), e))?;
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
                        ).await
                        .map_err(|e| pipeline::step_error("product format retry".into(), e))?;
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
                    crate::vfs::create_dir_all(parent).ok();
                }
                snapshot_pipeline_baseline(&store.out, gs, &p, &mut baselined, baseline_key)?;
                if crate::vfs::write(&full, format!("{}\n", content.trim_end())).is_ok() {
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
                    ).await
                    .map_err(|e| pipeline::step_error("product retry".into(), e))?;
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
            code.push('\n');
            code.push_str(&strip_fences(&reply));
        }
        code.push('\n');
    }
    if code.trim().is_empty() {
        return Err("empty product".into());
    }
    let product_path = gs.deliverable.join(&product_rel);
    if let Some(p) = product_path.parent() {
        crate::vfs::create_dir_all(p).ok();
    }
    snapshot_pipeline_baseline(&store.out, gs, &product_rel, &mut baselined, baseline_key)?;
    crate::vfs::write(&product_path, &code).map_err(|e| e.to_string())?;

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
        .ask(instructions, &tests_user, &format!("gen {}", id), "tests").await
        .map_err(|e| pipeline::step_error("tests file".into(), e))?;
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
                        crate::vfs::create_dir_all(parent).ok();
                    }
                    snapshot_pipeline_baseline(&store.out, gs, &p, &mut baselined, baseline_key)?;
                    if crate::vfs::write(&full, format!("{}\n", content.trim_end())).is_ok() {
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
                    ).await
                    .map_err(|e| pipeline::step_error("tests format retry".into(), e))?;
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
                    .ask(instructions, &retry, &format!("gen {}", id), "tests retry").await
                    .map_err(|e| pipeline::step_error("tests retry".into(), e))?;
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
                crate::vfs::create_dir_all(p).ok();
            }
            snapshot_pipeline_baseline(&store.out, gs, &tests_rel, &mut baselined, baseline_key)?;
            crate::vfs::write(&tests_path, &tests_code).map_err(|e| e.to_string())?;
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
        ).await
        .map_err(|e| pipeline::step_error("manifest".into(), e))?;
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
                ).await
                .map_err(|e| pipeline::step_error("manifest format retry".into(), e))?;
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
                    .and_then(|p| crate::vfs::read_to_string(gs.deliverable.join(p)).ok())
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
        let retry_reply = runner.ask(instructions, &retry, &format!("gen {}", id), "manifest retry").await;
        if let Err(reason) = &retry_reply {
            if crate::session_recovery::is_interruption(reason) { return Err(reason.clone()); }
        }
        if let Ok(reply2) = retry_reply {
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
            let owned = ledger.entities.values().any(|e| e.files.iter().any(|f| f == path));
            if owned || path == product_rel || path == tests_rel {
                continue;
            }
            let p = gs.deliverable.join(path);
            if let Some(parent) = p.parent() {
                crate::vfs::create_dir_all(parent).ok();
            }
            snapshot_pipeline_baseline(&store.out, gs, path, &mut baselined, baseline_key)?;
            crate::vfs::write(&p, content).map_err(|e| e.to_string())?;
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
                crate::vfs::create_dir_all(p).ok();
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
            crate::vfs::write(&crit_path, criteria).ok();
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
        let deliverable_wide = declared_support.contains(&n)
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
        .get(&slug_of(id))
        .and_then(|e| e.unattached.clone());
    let mut ws = Store::load(&store.out);
    let ops = choice_ops(&ws, id, &choices, unattached.as_ref());
    if !ops.is_empty() {
        ws.apply(
            ops,
            &crate::store::Commit::session(vec![format!("g:generate:{}", id)], 0, None),
        );
    }
    Ok(files.len())
}
