//! Project task definitions and provider-independent runtime calls.
//! Mirrors docs/compiler/task-runtime.md. Provider execution belongs to the host.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[cfg(not(target_arch = "wasm32"))]
#[path = "task_runtime_local.rs"]
mod local;
#[cfg(not(target_arch = "wasm32"))]
#[path = "task_runtime_remote.rs"]
mod remote;
#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "task_runtime_scoped_tests.rs"]
mod scoped_tests;

thread_local! { static CANCEL_FILE: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) }; }
pub struct CancellationScope(Option<std::path::PathBuf>);
impl CancellationScope {
    pub fn enter(path: Option<&Path>) -> Self {
        Self(CANCEL_FILE.with(|p| {
            let old = p.borrow().clone();
            if let Some(path) = path {
                *p.borrow_mut() = Some(path.to_owned());
            }
            old
        }))
    }
}
impl Drop for CancellationScope {
    fn drop(&mut self) {
        CANCEL_FILE.with(|p| *p.borrow_mut() = self.0.take());
    }
}
fn cancellation_file() -> Option<std::path::PathBuf> {
    CANCEL_FILE.with(|p| p.borrow().clone())
}
fn operation_cancelled() -> bool {
    cancellation_file().is_some_and(|p| crate::vfs::exists(&p))
}
#[cfg(not(target_arch = "wasm32"))]
pub fn cancel_owned(root: &Path, fence: &Path) -> Result<(), String> {
    local::cancel_owned(root, fence)?;
    if std::env::var_os("JAZYK_RUNTIME_URL").is_some() {
        remote::cancel_owned(root, fence)?;
    }
    Ok(())
}

pub const PROMPT: &str = include_str!("../../docs/compiler/skills/agent/execute-task.md");
pub const TOOLS: [&str; 9] = [
    "task_context",
    "update_tasks",
    "run_task",
    "inspect_run",
    "wait_run",
    "cancel_run",
    "read_run_log",
    "search_run_log",
    "read_run_changes",
];
pub const MAX_BYTES: usize = 48 * 1024 * 1024;
pub const MAX_FILES: usize = 8192;
fn image() -> String {
    "python:3.12-slim".into()
}
fn cwd() -> String {
    ".".into()
}
fn size() -> String {
    "small".into()
}
fn timeout() -> u64 {
    1800
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub description: String,
    pub command: String,
    #[serde(default = "image")]
    pub image: String,
    #[serde(default = "cwd")]
    pub cwd: String,
    #[serde(default = "size")]
    pub size: String,
    #[serde(default = "timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub products: Vec<String>,
    #[serde(default)]
    pub cache: Vec<String>,
    #[serde(default)]
    pub reports: Vec<String>,
}
impl Task {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_id(&self.id) {
            return Err(
                "Task IDs need 1 to 60 ASCII letters, digits, underscores, or hyphens.".into(),
            );
        }
        for (name, value, cap) in [
            ("name", &self.name, 120),
            ("description", &self.description, 4000),
            ("command", &self.command, 16384),
            ("image", &self.image, 512),
        ] {
            if value.trim().is_empty() || value.len() > cap || value.contains('\0') {
                return Err(format!(
                    "Task {name} is empty, too long, or contains a NUL."
                ));
            }
        }
        if self.image.chars().any(char::is_whitespace) || self.image.starts_with('-') {
            return Err("Invalid image reference.".into());
        }
        if self.cwd != "." && (!safe_path(&self.cwd) || protected(&self.cwd)) {
            return Err(format!("Task {} cwd {:?} must be a project-relative directory; use \".\" for the project root, not an absolute deliverable path.", self.id, self.cwd));
        }
        if !matches!(self.size.as_str(), "small" | "medium" | "large")
            || !(1..=7200).contains(&self.timeout_seconds)
        {
            return Err(
                "Choose small, medium, or large and a timeout from 1 to 7200 seconds.".into(),
            );
        }
        if self.env.len() > 40 {
            return Err("Too many environment values.".into());
        }
        for (k, v) in &self.env {
            if k.is_empty()
                || k.len() > 100
                || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || k.starts_with("AWS_")
                || k.starts_with("JAZYK_")
                || v.len() > 4096
                || v.contains('\0')
            {
                return Err("Invalid or reserved environment variable.".into());
            }
        }
        let mut seen = BTreeMap::new();
        for (field, patterns) in [
            ("products", &self.products),
            ("cache", &self.cache),
            ("reports", &self.reports),
        ] {
            if patterns.len() > 40 {
                return Err("Too many file patterns.".into());
            }
            for pattern in patterns {
                if !safe_path(pattern) || protected(pattern) || pattern.starts_with('!') {
                    return Err(format!("Task {} {field} pattern {pattern:?} must be an unprotected, project-relative glob.", self.id));
                }
                if let Some(previous) = seen.insert(pattern, field) {
                    return Err(format!("Task {} {field} pattern {pattern:?} repeats a pattern in {previous}. Keep each pattern in only one field; put test report patterns in reports.", self.id));
                }
                globset::GlobBuilder::new(pattern)
                    .literal_separator(true)
                    .build()
                    .map_err(|e| format!("Task {} {field} pattern {pattern:?}: {e}", self.id))?;
            }
        }
        Ok(())
    }
    pub fn resources(&self) -> (u32, u32) {
        match self.size.as_str() {
            "medium" => (2, 4),
            "large" => (4, 8),
            _ => (1, 2),
        }
    }
    pub fn hash(&self) -> String {
        digest(&serde_json::to_vec(self).unwrap())
    }
}
pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 60
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
pub fn safe_path(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 512
        && !s.starts_with('/')
        && !s.contains(['\\', '\0', ':', '\n', '\r'])
        && s.split('/').all(|p| !matches!(p, "" | "." | ".."))
}
pub fn protected(s: &str) -> bool {
    s.starts_with(".jazyk/")
        || s == ".jazyk"
        || s.split('/').any(|p| {
            matches!(
                p,
                ".jazyk" | ".git" | ".ssh" | ".aws" | ".env" | "id_rsa" | "id_ed25519" | ".worktrees"
            ) || p.ends_with(".env")
                || p.ends_with(".pem")
                || p.ends_with(".key")
        })
}
pub fn input_allowed(s: &str) -> bool {
    safe_path(s) && (!protected(s) || matches!(s, ".jazyk/jazyk.toml" | ".jazyk/.gitignore"))
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn matches(patterns: &[String], path: &str) -> bool {
    patterns.iter().any(|p| {
        globset::GlobBuilder::new(p)
            .literal_separator(true)
            .build()
            .is_ok_and(|g| g.compile_matcher().is_match(path))
    })
}
pub fn definitions(source: &str) -> Result<Vec<Task>, String> {
    let value: toml::Value = toml::from_str(source).map_err(|e| e.to_string())?;
    let tasks: Vec<Task> = value
        .get("tasks")
        .cloned()
        .map(|v| v.try_into())
        .transpose()
        .map_err(|e: toml::de::Error| e.to_string())?
        .unwrap_or_default();
    validate(&tasks)?;
    Ok(tasks)
}
fn validate(tasks: &[Task]) -> Result<(), String> {
    if tasks.len() > 100 {
        return Err("A project supports at most 100 tasks.".into());
    }
    let mut ids = BTreeSet::new();
    for t in tasks {
        t.validate()?;
        if !ids.insert(&t.id) {
            return Err(format!("Duplicate task ID: {}", t.id));
        }
    }
    Ok(())
}
pub fn edit(source: &str, tasks: &[Task]) -> Result<String, String> {
    validate(tasks)?;
    let mut document = source
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| e.to_string())?;
    let mut entries = toml_edit::ArrayOfTables::new();
    for task in tasks {
        let fresh = toml_edit::ser::to_document(task).map_err(|e| e.to_string())?;
        let mut table = document
            .get("tasks")
            .and_then(|v| v.as_array_of_tables())
            .into_iter()
            .flat_map(|v| v.iter())
            .find(|t| t.get("id").and_then(|v| v.as_str()) == Some(task.id.as_str()))
            .cloned()
            .unwrap_or_default();
        for (key, value) in fresh.iter() {
            table.insert(key, value.clone());
        }
        entries.push(table);
    }
    document["tasks"] = toml_edit::Item::ArrayOfTables(entries);
    Ok(document.to_string())
}
pub fn load(root: &Path) -> Result<(String, Vec<Task>), String> {
    let source =
        crate::vfs::read_to_string(root.join(".jazyk/jazyk.toml")).map_err(|e| e.to_string())?;
    let tasks = definitions(&source)?;
    Ok((source, tasks))
}
pub(crate) fn may_contain_product(products: &[String], dir: &str) -> bool {
    let dir = format!("{dir}/");
    products.iter().any(|pattern| {
        let prefix = &pattern[..pattern
            .find(|c: char| matches!(c, '*' | '?' | '[' | '{'))
            .unwrap_or(pattern.len())];
        prefix.starts_with(&dir) || dir.starts_with(prefix)
    })
}
pub fn snapshot(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let products: Vec<String> = load(root)?.1.into_iter().flat_map(|t| t.products).collect();
    fn walk(
        root: &Path,
        dir: &Path,
        files: &mut BTreeMap<String, String>,
        bytes: &mut usize,
        products: &[String],
        product_only: bool,
    ) -> Result<(), String> {
        for entry in crate::vfs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if relative != ".jazyk" && !input_allowed(&relative) {
                continue;
            }
            #[cfg(not(target_arch = "wasm32"))]
            if !crate::vfs::is_scoped()
                && std::fs::symlink_metadata(&path)
                    .map_err(|e| e.to_string())?
                    .file_type()
                    .is_symlink()
            {
                continue;
            }
            if entry.is_dir() {
                if crate::vfs::exists(&path.join(".jazyk/jazyk.toml")) {
                    continue;
                }
                let product_only = product_only
                    || matches!(
                        entry.file_name().to_str(),
                        Some("node_modules" | "target" | ".cache")
                    );
                if product_only && !may_contain_product(products, &relative) {
                    continue;
                }
                walk(root, &path, files, bytes, products, product_only)?;
            } else {
                if product_only && !matches(products, &relative) {
                    continue;
                }
                let mut data = crate::vfs::read(&path).map_err(|e| e.to_string())?;
                if relative == ".jazyk/jazyk.toml" {
                    let mut config: toml::Value = toml::from_str(&String::from_utf8_lossy(&data))
                        .map_err(|e| e.to_string())?;
                    fn redact(value: &mut toml::Value) {
                        match value {
                            toml::Value::Table(t) => {
                                t.retain(|k, _| {
                                    !matches!(
                                        k,
                                        "api_key"
                                            | "password"
                                            | "secret"
                                            | "access_token"
                                            | "refresh_token"
                                    )
                                });
                                for (_, v) in t.iter_mut() {
                                    redact(v);
                                }
                            }
                            toml::Value::Array(a) => {
                                for v in a {
                                    redact(v);
                                }
                            }
                            _ => {}
                        }
                    }
                    redact(&mut config);
                    data = toml::to_string(&config)
                        .map_err(|e| e.to_string())?
                        .into_bytes();
                }
                *bytes += data.len();
                if *bytes > MAX_BYTES || files.len() >= MAX_FILES {
                    return Err("Runtime input exceeds 48 MiB or 8192 files.".into());
                }
                files.insert(relative, STANDARD.encode(data));
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files, &mut 0, &products, false)?;
    Ok(files)
}
pub fn input_hash(files: &BTreeMap<String, String>) -> String {
    digest(&serde_json::to_vec(files).unwrap())
}
fn legacy_tasks(source: &str) -> Vec<Task> {
    let Ok(value) = toml::from_str::<toml::Value>(source) else {
        return Vec::new();
    };
    value.get("deliveries").and_then(|v|v.as_table()).into_iter().flat_map(|v|v.iter()).filter_map(|(id,value)|{
        if value.get("kind").and_then(|v|v.as_str())!=Some("script"){return None;}
        let command=value.get("command")?.as_str()?;
        serde_json::from_value(json!({"id":id,"name":id,"description":format!("Converted legacy delivery {id}. Review its image and report bindings before running."),"command":command,"reports":value.get("reports").cloned().unwrap_or(toml::Value::Array(vec![]))})).ok()
    }).collect()
}
pub fn call(root: &Path, name: &str, args: &Value) -> Result<Value, String> {
    call_impl(root, name, args, true)
}

thread_local! { static SWEEP_DEFERRED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
// A workflow sweeps once when it ends, so its compile action leaves the task list
// alone for the workflow's later actions.
pub struct DeferSweep(bool);
impl DeferSweep {
    pub fn enter() -> Self { Self(SWEEP_DEFERRED.with(|d| d.replace(true))) }
}
impl Drop for DeferSweep {
    fn drop(&mut self) { SWEEP_DEFERRED.with(|d| d.set(self.0)); }
}
pub fn sweep_deferred() -> bool { SWEEP_DEFERRED.with(|d| d.get()) }

// Record what a ledger session's update_tasks did to the tasks it owns: an added
// task becomes the session's with its definition hash; an owned task the session
// edits keeps ownership under its new hash; records of tasks no longer listed end.
// Mirrors docs/consumers/gen.md#session-tasks-are-swept-when-unused.
pub fn record_owned(out: &Path, before: &[Task], after: &[Task], owner: &str) -> Result<(), String> {
    let mut ledger = crate::gen::Ledger::load(out);
    if let Some(why) = &ledger.unreadable { return Err(format!("The ledger did not load: {why}")); }
    let original = ledger.tasks.clone();
    ledger.tasks.retain(|id, _| after.iter().any(|t| t.id == *id));
    for task in after {
        match before.iter().find(|t| t.id == task.id) {
            None => { ledger.tasks.insert(task.id.clone(), crate::gen::TaskOwner {
                created_by: owner.to_owned(), created_at: crate::verify::now_iso(), definition: task.hash() }); }
            Some(previous) => if let Some(record) = ledger.tasks.get_mut(&task.id) {
                if record.definition == previous.hash() { record.definition = task.hash(); }
            },
        }
    }
    if ledger.tasks != original { ledger.save(out); }
    Ok(())
}

// Every task id a ledger names: test and criterion runs, the build, and the
// tasks a generation record lists.
fn ledger_uses(ledger: &crate::gen::Ledger) -> std::collections::BTreeSet<String> {
    let id = |run: &str| run.strip_prefix("task:").map(|id| id.trim().to_owned());
    let mut used = std::collections::BTreeSet::new();
    for row in ledger.requirements.values() {
        used.extend(id(&row.test.run));
        used.extend(row.tests.values().filter_map(|t| id(&t.test.run)));
    }
    used.extend(ledger.build.as_ref().and_then(|b| id(&b.run)));
    used.extend(ledger.entities.values().flat_map(|e| e.tasks.iter().cloned()));
    used
}

// Remove session-owned tasks no ledger uses, through the configuration lock and
// revision check. A task is session-owned only while its definition still matches
// the recorded one, so an author's edit or re-creation makes it the author's. `own`
// is the sweeping build's out (its ledger counts even under --out); every other set
// with a session at work keeps its tasks. Returns the removed ids.
// Mirrors docs/consumers/gen.md#session-tasks-are-swept-when-unused.
pub fn sweep_unused(root: &Path, own: Option<&Path>) -> Result<Vec<String>, String> {
    let workspace = crate::document_sets::Workspace::load(root).map_err(|(_, e)| e)?;
    let mut outs = Vec::new();
    for set in &workspace.sets {
        outs.push(workspace.resolve(Some(&set.id)).map_err(|(_, e)| e)?.out);
    }
    if let Some(own) = own { if !outs.iter().any(|o| o == own) { outs.push(own.to_path_buf()); } }
    let mut ledgers = Vec::new();
    for out in &outs {
        let ledger = crate::gen::Ledger::load(out);
        // An unreadable ledger's references are unknown: nothing is provably unused.
        if ledger.unreadable.is_some() { return Ok(Vec::new()); }
        // A set with a session at work (a goal or entity lease, or another build)
        // may be about to bind the tasks it owns: they all count as used.
        let busy = Some(out.as_path()) != own
            && (!crate::control::leases(out).is_empty() || crate::control::build_lease(out).is_some());
        ledgers.push((out, ledger, busy));
    }
    if ledgers.iter().all(|(_, l, _)| l.tasks.is_empty()) { return Ok(Vec::new()); }
    let mut used: std::collections::BTreeSet<String> = ledgers.iter().flat_map(|(_, l, _)| ledger_uses(l)).collect();
    used.extend(ledgers.iter().filter(|(_, _, busy)| *busy).flat_map(|(_, l, _)| l.tasks.keys().cloned()));
    let (source, tasks) = load(root)?;
    let owned = |t: &Task| ledgers.iter().any(|(_, l, _)| l.tasks.get(&t.id).is_some_and(|o| o.definition == t.hash()));
    let removed: Vec<String> = tasks.iter().filter(|t| !used.contains(&t.id) && owned(t)).map(|t| t.id.clone()).collect();
    if removed.is_empty() { return Ok(removed); }
    let kept: Vec<Task> = tasks.iter().filter(|t| !removed.contains(&t.id)).cloned().collect();
    let next = edit(&source, &kept)?;
    crate::document_sets::atomic_config(root, &next, &crate::model::hash_hex(&source))
        .map_err(|(_, error)| error)?;
    // The removed tasks' records end, so an author who restores one owns it. Only
    // idle sets own a removed task, and no session writes an idle set's ledger.
    for (out, _, _) in ledgers.iter().filter(|(_, l, busy)| !busy && removed.iter().any(|id| l.tasks.contains_key(id))) {
        let mut fresh = crate::gen::Ledger::load(out);
        if fresh.unreadable.is_some() { continue; }
        fresh.tasks.retain(|id, _| !removed.contains(id));
        fresh.save(out);
    }
    Ok(removed)
}

fn call_impl(root: &Path, name: &str, args: &Value, import_terminal: bool) -> Result<Value, String> {
    if !TOOLS.contains(&name) {
        return Err("Unknown runtime operation.".into());
    }
    if !matches!(name, "task_context" | "update_tasks" | "run_task") {
        let mut request = args.clone();
        if !request.is_object() {
            request = json!({});
        }
        request["operation"] = json!(name);
        let value = dispatch(root, &request)?;
        return finish_status(root, name, args, value, import_terminal);
    }
    let (source, tasks) = load(root)?;
    if name == "update_tasks" {
        if args["expected_revision"].as_str() != Some(digest(source.as_bytes()).as_str()) {
            return Err("Task settings changed. Reload task_context before saving.".into());
        }
        let tasks: Vec<Task> =
            serde_json::from_value(args["tasks"].clone()).map_err(|e| e.to_string())?;
        let next = edit(&source, &tasks)?;
        crate::document_sets::atomic_config(root, &next, &crate::model::hash_hex(&source))
            .map_err(|(_, error)| error)?;
        return Ok(json!({"tasks":tasks,"revision":digest(next.as_bytes())}));
    }
    let mut request = args.clone();
    if !request.is_object() {
        request = json!({});
    }
    request["operation"] = json!(name);
    // Waiting is the engine's choice, never part of the submitted request.
    let wait = name == "run_task" && args["wait"] == true;
    if let Some(fields) = request.as_object_mut() { fields.remove("wait"); fields.remove("task_ids"); }
    let mut wait_seconds = 0;
    if name == "run_task" {
        if operation_cancelled() {
            return Err("The owning operation was cancelled.".into());
        }
        if let Some(path) = cancellation_file() {
            request["cancel_file"] = json!(path);
        }
        let id = args["task_id"].as_str().ok_or("task_id is required")?;
        let mut task = tasks
            .iter()
            .find(|t| t.id == id)
            .ok_or("Task not found")?
            .clone();
        if let Some(size) = args["size"].as_str() {
            task.size = size.into();
        }
        if let Some(seconds) = args["timeout_seconds"].as_u64() {
            task.timeout_seconds = seconds;
        };
        task.validate()?;
        wait_seconds = task.timeout_seconds + 600;
        if args["expected_revision"].as_str() != Some(digest(source.as_bytes()).as_str()) {
            return Err("Task settings changed. Reload task_context before running.".into());
        }
        let key = args["operation_key"]
            .as_str()
            .ok_or("operation_key is required")?;
        if !valid_id(key) {
            return Err("Invalid operation key.".into());
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !crate::vfs::is_scoped() && std::env::var_os("JAZYK_RUNTIME_URL").is_some() {
            remote::validate_host(root)?;
        }
        let files = snapshot(root)?;
        request["input_digest"] = json!(input_hash(&files));
        request["files"] = json!(files);
        request["task"] = json!(task);
    }
    let selection = if name == "task_context" { Some(select_tasks(tasks, &args["task_ids"])?) } else { None };
    let host = dispatch(root, &request).and_then(|value| {
        if name == "run_task" && crate::vfs::is_scoped() {
            save_submission(root, &request, &value)?;
        }
        finish_status(root, name, args, value, import_terminal)
    });
    if wait {
        return host.and_then(|status| wait_terminal(root, status, wait_seconds, import_terminal));
    }
    if let Some((tasks, index)) = selection {
        let mut context = json!({"tasks":tasks,"legacy":legacy_tasks(&source),"revision":digest(source.as_bytes()),"projectRoot":root.to_string_lossy(),"execution":host.unwrap_or_else(|e|json!({"available":false,"reason":e})),"defaults":{"image":image(),"cwd":".","size":"small","timeout_seconds":timeout()}});
        if let Some(index) = index { context["index"] = index; }
        return Ok(context);
    }
    host
}

// Selected definitions in full, the rest by name. No selection keeps the full list.
fn select_tasks(tasks: Vec<Task>, selection: &Value) -> Result<(Vec<Task>, Option<Value>), String> {
    if selection.is_null() { return Ok((tasks, None)); }
    let ids: Vec<&str> = selection.as_array().ok_or("task_ids must be a list of task IDs")?
        .iter().map(|id| id.as_str().ok_or("task_ids must be a list of task IDs")).collect::<Result<_, _>>()?;
    if let Some(unknown) = ids.iter().find(|id| !tasks.iter().any(|t| t.id == **id)) {
        return Err(format!("Unknown task ID '{unknown}' in task_ids. Omit task_ids for the full list."));
    }
    let (selected, rest): (Vec<Task>, Vec<Task>) = tasks.into_iter().partition(|t| ids.contains(&t.id.as_str()));
    let index = rest.iter().map(|t| json!({"id":t.id,"name":t.name,"description":t.description})).collect();
    Ok((selected, Some(Value::Array(index))))
}

// Poll in code so a long run costs no inference per state change. Every early
// exit is an ordinary status reply: a suspended worker records it as the tool's
// receipt instead of leaving an uncertain pending call. The run keeps its deadline.
fn wait_terminal(root: &Path, mut status: Value, limit_seconds: u64, import_terminal: bool) -> Result<Value, String> {
    let id = status["id"].as_str().ok_or("Run ID missing")?.to_owned();
    let deadline = crate::sys::now_unix() + limit_seconds;
    fn cancelled() -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        { crate::hosted::cancelled() || operation_cancelled() }
        #[cfg(target_arch = "wasm32")]
        { operation_cancelled() }
    }
    while status["terminated"] != true {
        if cancelled() { return Ok(cancel_waiting(root, &id, status, import_terminal)); }
        if crate::session_recovery::suspension_requested() || crate::sys::now_unix() >= deadline {
            break;
        }
        match call_impl(root, "wait_run", &json!({"run_id":id,"wait_ms":5000}), import_terminal) {
            Ok(next) => status = next,
            // A hosted poll fails when the execution is suspended or cancelled
            // mid-wait. Any failure keeps the submitted run's identity.
            Err(error) => {
                if cancelled() { return Ok(cancel_waiting(root, &id, status, import_terminal)); }
                status["wait_error"] = json!(error);
                break;
            }
        }
    }
    Ok(status)
}

fn cancel_waiting(root: &Path, id: &str, mut status: Value, import_terminal: bool) -> Value {
    match call_impl(root, "cancel_run", &json!({"run_id":id}), import_terminal) {
        Ok(stopping) => stopping,
        Err(error) => { status["wait_error"] = json!(error); status }
    }
}

// The host owns execution. The active scoped worker owns publication into its
// private snapshot, which its existing lease and checkpoint publish atomically.
#[derive(Serialize, Deserialize)]
struct Submission {
    task: Task,
    input_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    import: Option<Value>,
}

fn submission_key(root: &Path, id: &str) -> Result<String, String> {
    if !valid_id(id) { return Err("Invalid run ID in submission receipt.".into()); }
    Ok(format!("task-submission:{}:{id}", root.display()))
}

fn save_submission(root: &Path, request: &Value, status: &Value) -> Result<(), String> {
    if !crate::session_recovery::active() { return Ok(()); }
    let id = status["id"].as_str().ok_or("Run ID missing")?;
    let key = submission_key(root, id)?;
    let receipt = Submission {
        task: serde_json::from_value(request["task"].clone()).map_err(|e| e.to_string())?,
        input_digest: request["input_digest"].as_str().ok_or("Task input identity missing")?.into(),
        import: None,
    };
    if let Some(previous) = crate::session_recovery::parent(&key)? {
        let previous: Submission = serde_json::from_value(previous).map_err(|e| e.to_string())?;
        if previous.task != receipt.task || previous.input_digest != receipt.input_digest {
            return Err("Run receipt identifies a different submitted task or input.".into());
        }
        return Ok(());
    }
    crate::session_recovery::set_parent(&key, serde_json::to_value(receipt).unwrap());
    Ok(())
}

fn import_scoped_result(root: &Path, id: &str, result: Option<&Value>, terminal_state: Option<&str>) -> Result<Option<Value>, String> {
    if !crate::session_recovery::active() { return Ok(None); }
    let key = submission_key(root, id)?;
    let Some(value) = crate::session_recovery::parent(&key)? else { return Ok(None) };
    let mut receipt: Submission = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if let Some(import) = receipt.import { return Ok(Some(import)); }
    let fetched;
    let result = if let Some(result) = result { result } else {
        fetched = dispatch(root, &json!({"operation":"result","run_id":id}))?;
        &fetched
    };
    if terminal_state.is_some_and(|state| result["status"].as_str() != Some(state)) {
        return Err("Retained task result does not match its terminal run state.".into());
    }
    let input: BTreeMap<String, String> = serde_json::from_value(result["input_files"].clone())
        .map_err(|_| "Retained task result has no frozen inputs.")?;
    if input_hash(&input) != receipt.input_digest {
        return Err("Retained task result does not match its submitted input identity.".into());
    }
    let validated = validate_result(&receipt.task, &input, result.clone())?;
    let imported = apply_result(root, &receipt.task, &input, &validated, id)?;
    receipt.import = Some(imported.clone());
    crate::session_recovery::set_parent(&key, serde_json::to_value(receipt).unwrap());
    Ok(Some(imported))
}

fn finish_status(root: &Path, operation: &str, args: &Value, mut status: Value, import_terminal: bool) -> Result<Value, String> {
    if import_terminal && crate::vfs::is_scoped() && status["terminated"] == true
        && matches!(operation, "run_task" | "inspect_run" | "wait_run")
    {
        let id = status["id"].as_str().or_else(|| args["run_id"].as_str()).ok_or("Run ID missing")?;
        if let Some(import) = import_scoped_result(root, id, None, status["state"].as_str())? {
            let repeats_summary = status.get("result").is_none() || status.get("result") == status.get("summary");
            if !status["result"].is_object() {
                status["result"] = if status["summary"].is_object() { status["summary"].clone() } else { json!({}) };
            }
            status["result"]["import_pending"] = json!(false);
            status["result"]["import"] = import;
            if repeats_summary && status.get("summary").is_some() { status["summary"] = status["result"].clone(); }
        }
    }
    Ok(metadata_response(operation, status))
}
// Enforce the response contract at the common boundary, including adapters that
// return saved state directly for history, idempotent starts, or cancellation.
fn metadata_response(operation: &str, mut value: Value) -> Value {
    fn omit_log(value: &mut Value) {
        if let Some(fields) = value.as_object_mut() {
            fields.remove("log");
            for key in ["result", "summary"] {
                if let Some(nested) = fields.get_mut(key).and_then(Value::as_object_mut) {
                    nested.remove("log");
                }
            }
        }
    }
    if operation == "task_context" {
        if let Some(runs) = value.get_mut("runs").and_then(Value::as_array_mut) {
            for run in runs {
                omit_log(run);
            }
        }
    } else if matches!(
        operation,
        "run_task" | "inspect_run" | "wait_run" | "cancel_run"
    ) {
        omit_log(&mut value);
    }
    value
}

// A worker already knows the task definitions from task_context. Keep its status
// observations small without changing retained runtime evidence or HTTP views.
pub(crate) fn worker_response(operation: &str, mut value: Value) -> Value {
    fn status(value: &mut Value) {
        let Some(fields) = value.as_object_mut() else { return };
        if let Some(task) = fields.get_mut("task").and_then(Value::as_object_mut) {
            task.retain(|key, _| matches!(key.as_str(), "id" | "name"));
        }
        if fields.contains_key("result") && fields.get("summary") == fields.get("result") {
            fields.remove("summary");
        } else if !fields.contains_key("result") {
            if let Some(summary) = fields.remove("summary") {
                fields.insert("result".into(), summary);
            }
        }
    }
    if operation == "task_context" {
        if let Some(execution) = value.get_mut("execution").and_then(Value::as_object_mut) {
            let next = if let Some(runs) = execution.get_mut("runs").and_then(Value::as_array_mut) {
                let next = (runs.len() > 5).then(|| runs[4]["id"].clone());
                runs.truncate(5);
                for run in runs {
                    status(run);
                }
                next
            } else { None };
            if let Some(next) = next {
                execution.insert("next_cursor".into(), next);
            }
        }
    } else if matches!(operation, "run_task" | "inspect_run" | "wait_run" | "cancel_run") {
        status(&mut value);
    }
    value
}

pub fn integration_context(root: &Path) -> Result<Value, String> {
    #[cfg(not(target_arch = "wasm32"))]
    if crate::vfs::is_scoped() || std::env::var_os("JAZYK_RUNTIME_URL").is_some() {
        return dispatch(root, &json!({"operation":"project_context"}));
    }
    crate::project_context::local(root)
}

fn dispatch(root: &Path, request: &Value) -> Result<Value, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if crate::vfs::is_scoped() {
            crate::hosted::runtime(request)
        } else if std::env::var_os("JAZYK_RUNTIME_URL").is_some() {
            remote::request(root, request)
        } else {
            local::request(root, request)
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (root, request);
        Err("Connect a local engine or Cloud to execute tasks.".into())
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub fn remote_watch(root: &Path, id: &str) -> Result<(), String> {
    remote::watch(root, id)
}
#[cfg(not(target_arch = "wasm32"))]
pub fn supervise(path: &Path) -> Result<(), String> {
    local::supervise(path)
}
#[cfg(not(target_arch = "wasm32"))]
pub fn watchdog(path: &Path) -> Result<(), String> {
    local::watchdog(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    const CONFIG:&str="[llm]\nmodel='existing'\n# retained comment\n[[tasks]]\nid='test'\nname='Tests'\ndescription='Run unit tests'\ncommand='mvn test'\nreports=['target/*.xml']\n";

    #[test]
    fn worker_status_keeps_exact_evidence_without_repeated_task_definitions() {
        let result = json!({"status":"succeeded","complete":true,"exit_code":0,
            "tests":[{"name":"Greeter::hello","status":"passed","report":"reports/tests.xml"}],
            "file_count":2,"report_errors":[],"import_pending":false});
        let original = json!({"id":"run-1","task":{"id":"test","name":"Tests",
            "command":"mvn test","description":"Run unit tests","image":"test-image","reports":["reports/*.xml"]},
            "state":"succeeded","terminated":true,"cancel_requested":false,"deadline":123,
            "input_digest":"frozen-input","source_revision":17,"error":null,"result":result,"summary":result});
        for operation in ["run_task", "inspect_run", "wait_run", "cancel_run"] {
            let compact = worker_response(operation, original.clone());
            let mut expected = original.clone();
            expected["task"] = json!({"id":"test","name":"Tests"});
            expected.as_object_mut().unwrap().remove("summary");
            assert_eq!(compact, expected, "{operation}");
            assert_eq!(compact["result"], result);
            assert!(compact.to_string().len() < original.to_string().len());
        }
        assert_eq!(original["task"]["command"], "mvn test");
        assert_eq!(original["summary"], result);
        let start = json!({"id":"run-2","task":{"id":"test","command":"mvn test"},"summary":null});
        let start = worker_response("run_task", start);
        assert_eq!(start["result"], Value::Null);
        assert!(!start.as_object().unwrap().contains_key("summary"));
        let different = json!({"summary":{"provider":"local"},"result":result});
        assert_eq!(worker_response("inspect_run", different.clone()), different);
    }

    #[test]
    fn worker_context_pages_history_without_losing_current_tasks_or_cursors() {
        let tasks = json!([{"id":"test","name":"Tests","command":"mvn test","reports":["reports/*.xml"]}]);
        let runs: Vec<_> = (0..12).map(|n| json!({"id":format!("run-{n}"),
            "task":{"id":"test","name":"Tests","command":"mvn test"},"state":"succeeded", "summary":null})).collect();
        let original = json!({"tasks":tasks,"legacy":[],"revision":"configuration-revision",
            "projectRoot":"/project","defaults":{"cwd":"."},
            "execution":{"available":true,"location":"cloud","runs":runs,"next_cursor":"run-29"}});
        let first = worker_response("task_context", original.clone());
        for key in ["tasks", "legacy", "revision", "projectRoot", "defaults"] {
            assert_eq!(first[key], original[key], "{key}");
        }
        assert_eq!(first["execution"]["available"], true);
        assert_eq!(first["execution"]["runs"].as_array().unwrap().len(), 5);
        assert_eq!(first["execution"]["next_cursor"], "run-4");
        assert_eq!(original["execution"]["runs"].as_array().unwrap().len(), 12);
        let mut last = original;
        last["execution"]["runs"] = json!([{"id":"last","task":{"id":"test"},"summary":null}]);
        last["execution"]["next_cursor"] = Value::Null;
        assert_eq!(worker_response("task_context", last)["execution"]["next_cursor"], Value::Null);
        let unavailable = json!({"tasks":tasks,"execution":{"available":false,"reason":"No runtime attached."}});
        assert_eq!(worker_response("task_context", unavailable.clone()), unavailable);
    }

    #[test]
    fn explicit_products_survive_default_build_directory_omissions() {
        let root =
            std::env::temp_dir().join(format!("jazyk-product-input-{}", crate::sys::now_millis()));
        for dir in [".jazyk", "target", "node_modules", "nested/target"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::write(
            root.join(".jazyk/jazyk.toml"),
            format!("{CONFIG}products=['target/*.jar','**/target/*.pdf']\n"),
        )
        .unwrap();
        for path in [
            "target/book.jar",
            "target/disposable.bin",
            "node_modules/dependency.js",
            "nested/target/book.pdf",
        ] {
            std::fs::write(root.join(path), b"artifact").unwrap();
        }
        let files = snapshot(&root).unwrap();
        assert!(files.contains_key("target/book.jar"));
        assert!(files.contains_key("nested/target/book.pdf"));
        assert!(!files.contains_key("target/disposable.bin"));
        assert!(!files.contains_key("node_modules/dependency.js"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn command_not_script_and_roundtrip() {
        let tasks = definitions(CONFIG).unwrap();
        assert_eq!(tasks[0].command, "mvn test");
        let changed = edit(CONFIG, &tasks).unwrap();
        assert!(changed.contains("# retained comment"), "{changed}");
        assert!(changed.contains("existing"));
        assert_eq!(definitions(&changed).unwrap(), tasks);
    }
    #[test]
    fn task_selection_keeps_full_default_and_indexes_unselected_tasks() {
        let mut tasks = definitions(CONFIG).unwrap();
        let mut other = tasks[0].clone();
        other.id = "lint".into();
        other.name = "Lint".into();
        tasks.push(other);
        assert_eq!(select_tasks(tasks.clone(), &Value::Null).unwrap(), (tasks.clone(), None));
        let (selected, index) = select_tasks(tasks.clone(), &json!(["lint"])).unwrap();
        assert_eq!(selected, vec![tasks[1].clone()]);
        assert_eq!(index.unwrap(), json!([{"id":tasks[0].id,"name":tasks[0].name,"description":tasks[0].description}]));
        let (selected, index) = select_tasks(tasks.clone(), &json!([])).unwrap();
        assert!(selected.is_empty());
        assert_eq!(index.unwrap().as_array().unwrap().len(), 2);
        let error = select_tasks(tasks.clone(), &json!(["missing"])).unwrap_err();
        assert!(error.contains("'missing'"), "{error}");
        assert!(select_tasks(tasks, &json!("lint")).is_err());
    }
    #[test]
    fn sweep_removes_only_unused_session_tasks_and_keeps_author_tasks() {
        let root = std::env::temp_dir().join(format!("jazyk-task-sweep-{}-{}",
            crate::sys::process_id(), crate::sys::now_millis()));
        let task = |id: &str, command: &str| format!("[[tasks]]\nid='{id}'\nname='{id}'\ndescription='d'\ncommand='{command}'\n");
        let base = format!("version=1\n# kept comment\n[[sets]]\nid='ds_main'\nname='Main'\ndocs=['docs/**']\nsource=['src/**']\n{}", task("author", "true"));
        crate::vfs::create_dir_all(root.join(".jazyk")).unwrap();
        crate::vfs::write(root.join(".jazyk/jazyk.toml"), &base).unwrap();
        let out = crate::document_sets::Workspace::load(&root).unwrap().resolve(Some("ds_main")).unwrap().out;
        let row = |run: &str| format!("    entity: ent:page\n    test: {{kind: programmatic, label: l, artifact: t.py, name: n, run: '{run}'}}\n    hashes: {{requirement: r, test: t, files: f}}\n");
        crate::vfs::create_dir_all(out.join("gen")).unwrap();
        crate::vfs::write(out.join("gen/ledger.yaml"), format!("entities:\n  page: {{factHash: x, tasks: [util]}}\nrequirements:\n  req:a-1:\n{}  req:a-2:\n{}",
            row("task:suite"), row("task:author"))).unwrap();
        // A ledger session adds four tasks; they become its own with their hashes.
        let before = load(&root).unwrap().1;
        let session = format!("{base}{}{}{}{}", task("suite", "true"), task("old", "true"), task("util", "true"), task("adopted", "true"));
        crate::vfs::write(root.join(".jazyk/jazyk.toml"), &session).unwrap();
        let after = load(&root).unwrap().1;
        record_owned(&out, &before, &after, "g:bind:req:a-1").unwrap();
        let ledger = crate::gen::Ledger::load(&out);
        assert!(ledger.unreadable.is_none(), "{:?}", ledger.unreadable);
        assert_eq!(ledger.tasks.keys().cloned().collect::<Vec<_>>(), ["adopted", "old", "suite", "util"]);
        assert_eq!(ledger.tasks["old"].definition, after.iter().find(|t| t.id == "old").unwrap().hash());
        // The author edits "adopted": a different definition is the author's.
        crate::vfs::write(root.join(".jazyk/jazyk.toml"), session.replace("id='adopted'\nname='adopted'\ndescription='d'\ncommand='true'", "id='adopted'\nname='adopted'\ndescription='d'\ncommand='make lint'")).unwrap();
        // A set with a session at work keeps every task it owns.
        crate::control::claim(&out, "req:a-9", "worker-b").unwrap();
        assert!(sweep_unused(&root, Some(&root.join("other-build"))).unwrap().is_empty());
        crate::control::release_lease(&out, "req:a-9");
        // Only the unused session task goes, and its record ends with it.
        assert_eq!(sweep_unused(&root, None).unwrap(), vec!["old".to_string()]);
        let (text, kept) = load(&root).unwrap();
        assert_eq!(kept.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["author", "suite", "util", "adopted"]);
        assert!(text.contains("# kept comment"));
        assert!(!crate::gen::Ledger::load(&out).tasks.contains_key("old"));
        assert!(sweep_unused(&root, None).unwrap().is_empty(), "a second sweep converges");
        // An author who restores the removed task unchanged owns it.
        crate::vfs::write(root.join(".jazyk/jazyk.toml"), format!("{text}{}", task("old", "true"))).unwrap();
        assert!(sweep_unused(&root, None).unwrap().is_empty());
        crate::vfs::write(root.join(".jazyk/jazyk.toml"), &text).unwrap();
        // A build's own --out ledger counts as a use.
        crate::vfs::write(out.join("gen/ledger.yaml"), format!("tasks:\n  suite: {{createdBy: x, createdAt: x, definition: '{}'}}\n", kept[1].hash())).unwrap();
        let other = root.join("elsewhere");
        crate::vfs::create_dir_all(other.join("gen")).unwrap();
        crate::vfs::write(other.join("gen/ledger.yaml"), format!("requirements:\n  req:a-1:\n{}", row("task:suite"))).unwrap();
        assert!(sweep_unused(&root, Some(&other)).unwrap().is_empty());
        // An unreadable ledger proves nothing unused.
        crate::vfs::write(other.join("gen/ledger.yaml"), "tasks: [unclosed").unwrap();
        assert!(sweep_unused(&root, Some(&other)).unwrap().is_empty());
        assert_eq!(sweep_unused(&root, None).unwrap(), vec!["suite".to_string()]);
        crate::vfs::remove_dir_all(root).ok();
    }
    #[test]
    fn task_updates_share_the_config_lock_and_preserve_matcher_edits() {
        let root = std::env::temp_dir().join(format!("jazyk-task-cas-{}-{}",
            crate::sys::process_id(), crate::sys::now_millis()));
        crate::vfs::create_dir_all(root.join(".jazyk")).unwrap();
        let source = format!("version=1\n{CONFIG}");
        crate::vfs::write(root.join(".jazyk/jazyk.toml"), &source).unwrap();
        let mut tasks = definitions(&source).unwrap();
        tasks[0].description = "Run the ordinary project suite".into();
        let args = json!({"expected_revision":digest(source.as_bytes()),"tasks":tasks});
        // A task save and a matcher save cannot both pass independent locks.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let lock = crate::document_sets::ProjectLock::acquire(&root, "config.lock").unwrap();
            assert!(call(&root, "update_tasks", &args).unwrap_err().contains("another project operation"));
            assert_eq!(crate::vfs::read_to_string(root.join(".jazyk/jazyk.toml")).unwrap(), source);
            drop(lock);
        }
        let next = format!("{source}\n[[sets]]\nid='ds_main'\nname='Main'\ndocs=['docs/**']\nsource=['src/**']\n");
        crate::document_sets::atomic_config(&root, &next, &crate::model::hash_hex(&source)).unwrap();
        assert!(call(&root, "update_tasks", &args).unwrap_err().contains("settings changed"));
        assert_eq!(crate::vfs::read_to_string(root.join(".jazyk/jazyk.toml")).unwrap(), next);
        let saved = call(&root, "update_tasks", &json!({"expected_revision":digest(next.as_bytes()),"tasks":tasks})).unwrap();
        let (actual, actual_tasks) = load(&root).unwrap();
        assert_eq!(saved["revision"], digest(actual.as_bytes()));
        assert_eq!(actual_tasks, tasks);
        assert!(actual.contains("# retained comment"));
        let workspace = crate::document_sets::Workspace::load(&root).unwrap();
        assert_eq!(workspace.select("ds_main").unwrap().source, ["src/**"]);
        crate::vfs::remove_dir_all(root).ok();
    }

    #[test]
    fn bad_settings() {
        assert!(definitions(&CONFIG.replace("id='test'", "id='../bad'")).is_err());
        assert!(
            definitions(&CONFIG.replace("reports=['target/*.xml']", "cache=['../secret']"))
                .is_err()
        );
        let mut tasks = definitions(CONFIG).unwrap();
        tasks.push(tasks[0].clone());
        assert!(edit(CONFIG, &tasks).is_err());
    }
    #[test]
    fn protected_and_globs() {
        assert!(!input_allowed(".aws/credentials"));
        assert!(!input_allowed("src/.env"));
        assert!(input_allowed(".jazyk/jazyk.toml"));
        assert!(!input_allowed(".jazyk/ds_main/graph/a.yaml"));
        assert!(matches(
            &["target/**/*.xml".into()],
            "target/surefire/a.xml"
        ));
        assert!(!matches(&["target/*.xml".into()], "target/surefire/a.xml"));
    }

    #[test]
    fn invalid_task_fields_identify_the_repair_without_relaxing_validation() {
        let mut task = definitions(CONFIG).unwrap().remove(0);
        task.cwd = "/project".into();
        let error = task.validate().unwrap_err();
        assert!(error.contains("cwd") && error.contains("/project") && error.contains("\".\""));
        task.cwd = ".".into();
        task.products = vec!["reports/*.xml".into()];
        task.reports = task.products.clone();
        let error = task.validate().unwrap_err();
        assert!(
            error.contains("reports/*.xml")
                && error.contains("in products")
                && error.contains("put test report patterns in reports")
        );
        task.products.clear();
        assert!(task.validate().is_ok());
        task.cache = vec!["../outside".into()];
        let error = task.validate().unwrap_err();
        assert!(error.contains("cache") && error.contains("../outside"));
    }
}

/// A bounded result view shared by local and hosted adapters. Raw bytes stay behind
/// explicit path requests, never in the status poll response.
pub fn present(state: &Value, result: &Value, args: &Value) -> Result<Value, String> {
    let operation = args["operation"].as_str().unwrap_or("inspect_run");
    if matches!(operation, "inspect_run" | "wait_run") {
        let mut value = state.clone();
        if let Some(fields) = value.as_object_mut() {
            fields.remove("log");
        }
        if !result.is_null() {
            value["result"] = json!({"status":result["status"],"exit_code":result["exit_code"],"complete":result["complete"],"error":result["error"],"products":result["products"],"tests":result["tests"],"cache_skipped":result["cache_skipped"]==true,"report_errors":result["report_errors"],"file_count":result["files"].as_object().map(|v|v.len()).unwrap_or(0)});
        }
        return Ok(value);
    }
    if matches!(operation, "read_run_log" | "search_run_log") {
        let log = result["log"]
            .as_str()
            .or_else(|| state["log"].as_str())
            .unwrap_or("");
        let start = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(16000).min(64000) as usize;
        if operation == "search_run_log" {
            let query = args["query"].as_str().ok_or("query is required")?;
            if query.len() > 1024 {
                return Err("Search query too long.".into());
            }
            let lines: Vec<_> = log
                .lines()
                .enumerate()
                .skip(start)
                .filter(|(_, s)| s.contains(query))
                .take(limit.min(100))
                .map(|(i, s)| json!({"line":i+1,"text":s.chars().take(1000).collect::<String>()}))
                .collect();
            return Ok(json!({"matches":lines}));
        }
        let text: String = log.chars().skip(start).take(limit).collect();
        Ok(
            json!({"text":text,"next_offset":start+text.chars().count(),"complete":state["terminated"]}),
        )
    } else if operation == "read_run_changes" {
        let version = match args.get("version") {
            None => "changes",
            Some(value) => value.as_str().filter(|value| matches!(*value, "changes" | "input"))
                .ok_or("version must be changes or input")?,
        };
        if version == "input" && args["path"].as_str().is_none() {
            return Err("version: input requires path; omit version and path to list retained changes".into());
        }
        if let Some(path) = args["path"].as_str() {
            if !safe_path(path) || protected(path) {
                return Err("Invalid output path.".into());
            }
            let preview = |bytes: Option<Vec<u8>>| -> Value {
                match bytes {
                    None => json!({"encoding":"absent"}),
                    Some(bytes) if bytes.len() > 65536 => {
                        json!({"encoding":"binary","size":bytes.len(),"download":true})
                    }
                    Some(bytes) => match String::from_utf8(bytes) {
                        Ok(text) => json!({"encoding":"utf8","content":text}),
                        Err(error) => {
                            json!({"encoding":"binary","size":error.as_bytes().len(),"download":true})
                        }
                    },
                }
            };
            if version == "input" {
                let encoded = result["input_files"][path].as_str()
                    .ok_or("Retained input file not found; input reads use the frozen run snapshot, not the current workspace")?;
                let bytes = STANDARD.decode(encoded).map_err(|_| "Invalid retained input encoding")?;
                let mut value = if args["download"] == "1" {
                    json!({"encoding":"base64","content":encoded})
                } else { preview(Some(bytes)) };
                value["path"] = json!(path);
                value["version"] = json!("input");
                return Ok(value);
            }
            let content = result["files"][path]
                .as_str()
                .or_else(|| result["reports"][path].as_str());
            let before = result["input_files"][path]
                .as_str()
                .and_then(|s| STANDARD.decode(s).ok());
            let deleted = result["deleted"]
                .as_array()
                .is_some_and(|a| a.iter().any(|p| p == path));
            if content.is_none() && !deleted {
                return Err(if result["input_files"][path].is_string() {
                    "No retained change for this path. The frozen input exists: read_run_changes with this path and version: input reads it. Omit path to list retained changes"
                } else {
                    "Retained changed file not found. Omit path to list retained changes; version: input reads a path from the frozen run input when retained"
                }.into());
            }
            if args["download"] == "1" {
                return Ok(
                    json!({"path":path,"encoding":"base64","content":content.ok_or("Deleted file has no current contents")?}),
                );
            }
            let bytes = content
                .map(|s| STANDARD.decode(s))
                .transpose()
                .map_err(|e| e.to_string())?;
            let mut value = preview(bytes);
            value["path"] = json!(path);
            value["before"] = preview(before);
            value["change"] = json!(if deleted {
                "deleted"
            } else if result["input_files"].get(path).is_some() {
                "modified"
            } else {
                "created"
            });
            return Ok(value);
        }
        let retained: BTreeMap<_, _> = result["files"]
            .as_object()
            .into_iter()
            .flat_map(|m| m.iter())
            .chain(
                result["reports"]
                    .as_object()
                    .into_iter()
                    .flat_map(|m| m.iter()),
            )
            .collect();
        let paths:Vec<_>=retained.into_iter().map(|(path,bytes)|json!({"path":path,"size":bytes.as_str().map(|s|s.len()*3/4),"change":if result["input_files"].get(path).is_some(){"modified"}else{"created"}})).chain(result["deleted"].as_array().into_iter().flatten().map(|p|json!({"path":p,"change":"deleted"}))).skip(args["offset"].as_u64().unwrap_or(0)as usize).take(200).collect();
        let next = if paths.len() == 200 {
            Some(args["offset"].as_u64().unwrap_or(0) + 200)
        } else {
            None
        };
        Ok(json!({"files":paths,"complete":result["complete"],"next_offset":next}))
    } else {
        Err("Unsupported runtime operation.".into())
    }
}

pub fn api(
    root: &Path,
    method: &str,
    route: &str,
    body: &Value,
    path: &str,
) -> Option<Result<Value, String>> {
    let route = route.strip_prefix("/api").unwrap_or(route);
    let mut args = body.clone();
    if !args.is_object() {
        args = json!({});
    }
    if let Some((_, query)) = path.split_once('?') {
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            args[key.as_ref()] = if matches!(key.as_ref(), "offset" | "limit") {
                json!(value.parse::<u64>().unwrap_or(0))
            } else {
                json!(value)
            };
        }
    }
    let operation = match (method, route) {
        ("GET", "/tasks") => "task_context",
        ("PUT", "/tasks") => "update_tasks",
        ("GET", "/runs") => "task_context",
        ("POST", "/runs") => "run_task",
        _ => {
            let rest = route.strip_prefix("/runs/")?;
            let (id, action) = rest.split_once('/').unwrap_or((rest, ""));
            args["run_id"] = json!(id);
            match (method, action) {
                ("GET", "") => "inspect_run",
                ("POST", "cancel") => "cancel_run",
                ("GET", "log") => "read_run_log",
                ("GET", "search") => "search_run_log",
                ("GET", "changes") => "read_run_changes",
                _ => return Some(Err("Unsupported run route.".into())),
            }
        }
    };
    Some(call(root, operation, &args))
}

pub fn junit(bytes: &[u8]) -> Result<Vec<(String, String)>, String> {
    use quick_xml::events::Event;
    if bytes.len() > 1024 * 1024 {
        return Err(String::from("JUnit report exceeds its size limit."));
    }
    let mut reader = quick_xml::Reader::from_reader(bytes);
    reader.config_mut().check_end_names = true;
    let mut depth = 0usize;
    let mut cases = Vec::new();
    let mut current: Option<(String, String, usize)> = None;
    let mut root = false;
    loop {
        let event = reader
            .read_event()
            .map_err(|_| String::from("JUnit report is malformed XML."))?;
        match event {
            Event::DocType(_) => {
                return Err(String::from(
                    "JUnit reports cannot contain DTD declarations.",
                ))
            }
            Event::Start(ref tag) | Event::Empty(ref tag) => {
                let empty = matches!(event, Event::Empty(_));
                if !empty {
                    depth += 1;
                    if depth > 64 {
                        return Err(String::from("JUnit report nesting is too deep."));
                    }
                }
                let local = tag.local_name();
                let name = local.as_ref();
                if root && ((empty && depth == 0) || (!empty && depth == 1)) {
                    return Err(String::from(
                        "JUnit XML must contain exactly one document root.",
                    ));
                }
                if !root {
                    if !matches!(name, b"testsuites" | b"testsuite") {
                        return Err(String::from(
                            "Expected a JUnit testsuite or testsuites document.",
                        ));
                    }
                    root = true;
                }
                if name == b"testcase" {
                    if current.is_some() {
                        return Err(String::from("JUnit test cases cannot be nested."));
                    }
                    let mut class = String::new();
                    let mut test = None;
                    for attr in tag.attributes() {
                        let attr = attr.map_err(|_| String::from("Invalid JUnit attributes."))?;
                        let value = attr
                            .decode_and_unescape_value(reader.decoder())
                            .map_err(|_| String::from("Invalid JUnit attribute value."))?
                            .into_owned();
                        match attr.key.as_ref() {
                            b"classname" => class = value,
                            b"name" => test = Some(value),
                            _ => {}
                        }
                    }
                    let test = test.ok_or_else(|| String::from("JUnit testcase has no name."))?;
                    if class.len() + test.len() > 1024 {
                        return Err(String::from("JUnit test identity is too long."));
                    }
                    let case = (format!("{class}::{test}"), "passed".to_string());
                    if empty {
                        cases.push(case);
                    } else {
                        current = Some((case.0, case.1, depth));
                    }
                } else if let Some((_, status, _)) = &mut current {
                    if matches!(name, b"failure" | b"error") {
                        *status = "failed".into();
                    } else if name == b"skipped" && status != "failed" {
                        *status = "skipped".into();
                    }
                }
            }
            Event::End(tag) => {
                if tag.local_name().as_ref() == b"testcase" {
                    let Some((selector, status, case_depth)) = current.take() else {
                        return Err(String::from("Unexpected JUnit testcase end."));
                    };
                    if depth != case_depth {
                        return Err(String::from("Malformed JUnit testcase nesting."));
                    }
                    cases.push((selector, status));
                }
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| String::from("Malformed XML nesting."))?;
            }
            Event::Text(text) if depth == 0 && text.iter().any(|b| !b.is_ascii_whitespace()) => {
                return Err(String::from("Unexpected text outside the JUnit document."))
            }
            Event::Eof => break,
            _ => {}
        }
        if cases.len() > 10000 {
            return Err(String::from("JUnit report has too many test cases."));
        }
    }
    if !root || depth != 0 || current.is_some() {
        return Err(String::from("JUnit report is incomplete."));
    }
    Ok(cases)
}

/// Validate an untrusted worker manifest against its immutable submitted inputs.
/// A worker cannot promote arbitrary paths or smuggle cache into canonical files.
pub fn validate_result(
    task: &Task,
    input: &BTreeMap<String, String>,
    mut result: Value,
) -> Result<Value, String> {
    if !matches!(
        result["status"].as_str(),
        Some("succeeded" | "failed" | "cancelled" | "timed_out")
    ) || !result["complete"].is_boolean()
    {
        return Err("Invalid worker outcome.".into());
    }
    if result["log"]
        .as_str()
        .map_or(true, |s| s.len() > 2 * 1024 * 1024)
    {
        return Err("Invalid worker log.".into());
    }
    let mut total = 0;
    let mut count = 0;
    for kind in ["files", "reports", "cache"] {
        let map = result[kind].as_object().ok_or("Invalid retained files")?;
        for (path, encoded) in map {
            if !safe_path(path) || protected(path) {
                return Err("Protected output path.".into());
            }
            let permitted = match kind {
                "cache" => {
                    !input.contains_key(path)
                        && matches(&task.cache, path)
                        && !matches(&task.products, path)
                        && !matches(&task.reports, path)
                }
                "reports" => matches(&task.reports, path),
                _ => {
                    input.contains_key(path)
                        || matches(&task.products, path)
                        || matches(&task.reports, path)
                }
            };
            if !permitted {
                return Err("Output is outside the submitted retention patterns.".into());
            }
            let data = STANDARD
                .decode(encoded.as_str().ok_or("Invalid retained encoding")?)
                .map_err(|_| "Invalid retained encoding")?;
            total += data.len();
            count += 1;
            if total > MAX_BYTES || count > MAX_FILES {
                return Err("Output exceeds retention limits.".into());
            }
        }
    }
    for path in result["deleted"]
        .as_array()
        .ok_or("Invalid deletion list")?
    {
        let path = path.as_str().ok_or("Invalid deletion path")?;
        if !safe_path(path) || protected(path) || !input.contains_key(path) {
            return Err("Invalid deletion path.".into());
        }
    }
    for path in result["products"]
        .as_array()
        .ok_or("Invalid product list")?
    {
        let path = path.as_str().ok_or("Invalid product path")?;
        if !matches(&task.products, path) || result["files"].get(path).is_none() {
            return Err("Invalid product path.".into());
        }
    }
    let mut cases = BTreeMap::<String, Value>::new();
    let mut errors = Vec::new();
    for (path, encoded) in result["reports"].as_object().unwrap() {
        let bytes = STANDARD.decode(encoded.as_str().unwrap()).unwrap();
        match junit(&bytes) {
            Ok(found) => {
                for (identity, status) in found {
                    let duplicate = cases.contains_key(&identity);
                    cases.insert(identity.clone(),json!({"identity":identity,"status":if duplicate{"ambiguous"}else{&status},"report":path}));
                }
            }
            Err(error) => errors.push(json!({"report":path,"error":error})),
        }
    }
    result["tests"] = json!(cases.into_values().collect::<Vec<_>>());
    result["report_errors"] = json!(errors);
    if result["status"] != "succeeded" || result["complete"] != true {
        result["cache"] = json!({});
    }
    Ok(result)
}

pub fn project_root(path: &Path) -> Result<std::path::PathBuf, String> {
    for root in path.ancestors() {
        if crate::vfs::read_to_string(root.join(".jazyk/jazyk.toml")).is_ok() {
            return Ok(root.to_path_buf());
        }
    }
    Err("No project task configuration was found.".into())
}
/// Execute a saved finite task and wait through the shared adapter. Cancellation
/// remains an out-of-band operation while this call waits.
pub fn execute_with_key(root: &Path, id: &str, identity: &str) -> Result<(String, Value), String> {
    #[cfg(not(target_arch = "wasm32"))]
    if crate::vfs::is_scoped() { return execute_recoverable(root, id, identity); }
    let _ = identity;
    execute(root, id)
}

#[cfg(not(target_arch = "wasm32"))]
fn execute_recoverable(root: &Path, id: &str, identity: &str) -> Result<(String, Value), String> {
    use crate::session_recovery as recovery;
    #[derive(Serialize, Deserialize)]
    struct Pending {
        version: u32,
        context: Value,
        operation_key: String,
        deadline: u64,
        run_id: Option<String>,
        result: Option<Value>,
    }
    fn save(key: &str, pending: &Pending) -> Result<(), String> {
        recovery::set_parent(key, serde_json::to_value(pending).map_err(|e| e.to_string())?);
        recovery::checkpoint_parent().map_err(|e| format!("checkpoint-failed: {e}"))
    }
    let key = recovery::task_key(&format!("task:{}:{id}:{identity}",root.display()));
    let mut pending: Pending = match recovery::parent(&key)? {
        Some(value) => serde_json::from_value(value).map_err(|e| format!("checkpoint-incompatible: task continuation: {e}"))?,
        None => Pending {
            version: 1, context: call(root, "task_context", &json!({}))?,
            operation_key: format!("resume-{}", &digest(format!("{key}:{}:{}",crate::sys::now_millis(),crate::sys::process_id()).as_bytes())[..32]),
            deadline: crate::sys::now_unix() + 8100, run_id: None, result: None,
        },
    };
    if pending.version != 1 { return Err("checkpoint-incompatible: unsupported task continuation".into()); }
    if let (Some(run_id), Some(result)) = (&pending.run_id, &pending.result) {
        return Ok((run_id.clone(), result.clone()));
    }
    save(&key, &pending)?;
    if recovery::suspension_requested() { return Err("suspended".into()); }
    if pending.run_id.is_none() {
        let run = call_impl(root, "run_task", &json!({"task_id":id,"expected_revision":pending.context["revision"],"operation_key":pending.operation_key}), false)
            .map_err(|error| format!("execution-uncertain: Task submission needs its saved destination receipt: {error}"))?;
        pending.run_id = Some(run["id"].as_str().ok_or("Run ID missing")?.to_owned());
        save(&key, &pending)?;
    }
    let run_id = pending.run_id.clone().unwrap();
    loop {
        if crate::hosted::cancelled() || operation_cancelled() {
            let _ = call(root, "cancel_run", &json!({"run_id":run_id}));
            return Err(format!("Execution cancelled; run {run_id} is stopping."));
        }
        if recovery::suspension_requested() { save(&key,&pending)?; return Err("suspended".into()); }
        let status = call_impl(root, "wait_run", &json!({"run_id":run_id,"wait_ms":1000}), false)?;
        if status["terminated"] == true {
            let result = dispatch(root, &json!({"operation":"result","run_id":run_id}))?;
            apply_execution_result(root, id, &run_id, &pending.context, &status, &result)?;
            pending.result = Some(result.clone());
            save(&key,&pending)?;
            return Ok((run_id,result));
        }
        if crate::sys::now_unix() >= pending.deadline {
            let _ = call(root, "cancel_run", &json!({"run_id":run_id}));
            return Err(format!("Run {run_id} exceeded the waiting limit and was cancelled."));
        }
        crate::sys::sleep(std::time::Duration::from_millis(1000));
    }
}

pub fn execute(root: &Path, id: &str) -> Result<(String, Value), String> {
    let context = call(root, "task_context", &json!({}))?;
    let key = format!(
        "run-{}",
        &digest(
            format!(
                "{}:{}:{}",
                crate::sys::now_millis(),
                crate::sys::process_id(),
                id
            )
            .as_bytes()
        )[..32]
    );
    let run = call_impl(
        root,
        "run_task",
        &json!({"task_id":id,"expected_revision":context["revision"],"operation_key":key}),
        false,
    )?;
    let run_id = run["id"].as_str().ok_or("Run ID missing")?.to_owned();
    let deadline = crate::sys::now_unix() + 8100;
    loop {
        #[cfg(not(target_arch = "wasm32"))]
        if crate::hosted::cancelled() || operation_cancelled() {
            let _ = call(root, "cancel_run", &json!({"run_id":run_id}));
            return Err(format!("Execution cancelled; run {run_id} is stopping."));
        }
        let status = call_impl(root, "wait_run", &json!({"run_id":run_id,"wait_ms":1000}), false)?;
        if status["terminated"] == true {
            let result = dispatch(root, &json!({"operation":"result","run_id":run_id}))?;
            apply_execution_result(root, id, &run_id, &context, &status, &result)?;
            return Ok((run_id, result));
        }
        if crate::sys::now_unix() >= deadline {
            let _ = call(root, "cancel_run", &json!({"run_id":run_id}));
            return Err(format!(
                "Run {run_id} exceeded the waiting limit and was cancelled."
            ));
        }
        crate::sys::sleep(std::time::Duration::from_millis(1000));
    }
}

fn apply_execution_result(root: &Path, task_id: &str, run_id: &str, context: &Value, status: &Value, result: &Value) -> Result<(), String> {
    if crate::vfs::is_scoped() && import_scoped_result(root, run_id, Some(result), status["state"].as_str())?.is_some() {
        return Ok(());
    }
    // Local adapters already import during finalization. Saved continuations
    // from before submission receipts use their original frozen task context.
    if let (Ok(input), Some(definition)) = (
        serde_json::from_value::<BTreeMap<String, String>>(result["input_files"].clone()),
        context["tasks"].as_array().and_then(|tasks| tasks.iter().find(|task| task["id"] == task_id)),
    ) {
        let task: Task = serde_json::from_value(definition.clone()).map_err(|e| e.to_string())?;
        apply_result(root, &task, &input, result, run_id)?;
    }
    Ok(())
}
pub(crate) fn case_verdict(result: &Value, name: &str) -> (&'static str, String) {
    let found: Vec<_> = result["tests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| {
            let identity = t["identity"].as_str().unwrap_or("");
            identity == name
                || (!name.contains("::")
                    && identity.rsplit_once("::").is_some_and(|(_, n)| n == name))
        })
        .collect();
    if found.len() != 1 {
        return (
            "none",
            if found.is_empty() {
                "unknown: no matching result"
            } else {
                "ambiguous: multiple matching cases"
            }
            .into(),
        );
    }
    let status = found[0]["status"].as_str().unwrap_or("unknown");
    (
        match status {
            "passed" => "pass",
            "failed" => "fail",
            _ => "none",
        },
        status.into(),
    )
}
/// Import reports before products, with input equality checked by the caller.
pub fn import_evidence(
    store: &crate::store::Store,
    gs: &crate::gen::GenSettings,
    task: &Task,
    result: &Value,
    run_id: &str,
) -> usize {
    let mut ledger = crate::gen::Ledger::load(&store.out);
    let mut count = 0;
    let reference = format!("task:{}", task.id);
    for row in ledger.requirements.values_mut() {
        if (row.test.kind == "programmatic" && row.test.run == reference)
            || row.tests.values().any(|t| t.test.kind == "programmatic" && t.test.run == reference)
        {
            let files = crate::gen::hash_implementing(&store.out, gs, &row.files);
            crate::verify::rebaseline_files(row, files);
        }
        let mut touched = false;
        for test in row
            .tests
            .values_mut()
            .filter(|t| t.test.kind == "programmatic" && t.test.run == reference)
        {
            let (verdict, observed) = case_verdict(result, &test.test.name);
            test.verdict = verdict.into();
            test.last_run = Some(crate::verify::now_iso());
            test.exit_code = result["exit_code"].as_i64().map(|n| n as i32);
            test.evidence = Some(format!("run {run_id}: {observed}"));
            test.test_hash =
                crate::gen::hash_file(&crate::gen::artifact_path(&store.out, gs, &test.test));
            touched = true;
            count += 1;
        }
        if row.test.kind == "programmatic" && row.test.run == reference {
            let (verdict, observed) = case_verdict(result, &row.test.name);
            row.verdict = verdict.into();
            row.evidence = Some(format!("run {run_id}: {observed}"));
            row.last_run = Some(crate::verify::now_iso());
            row.exit_code = result["exit_code"].as_i64().map(|n| n as i32);
            row.hashes.test =
                crate::gen::hash_file(&crate::gen::artifact_path(&store.out, gs, &row.test));
            touched = true;
            count += 1;
        }
        if touched {
            row.hashes.files = crate::gen::hash_implementing(&store.out, gs, &row.files);
        }
    }
    if count > 0 {
        ledger.save(&store.out);
    }
    count
}
pub fn apply_result(
    root: &Path,
    task: &Task,
    input: &BTreeMap<String, String>,
    result: &Value,
    id: &str,
) -> Result<Value, String> {
    if input_hash(&snapshot(root)?) != input_hash(input) {
        return Ok(
            json!({"applied":false,"reason":"Project changed since submission. Results remain in run history."}),
        );
    }
    // Any command mutation of tested inputs makes its report an untested candidate.
    let changed_input = result["files"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.iter())
        .any(|(p, v)| {
            input.contains_key(p)
                && input.get(p).map(|s| json!(s)) != Some(v.clone())
                && !matches(&task.reports, p)
                && !matches(&task.products, p)
        });
    let deleted_input = result["deleted"].as_array().is_some_and(|v| {
        v.iter()
            .filter_map(Value::as_str)
            .any(|p| !matches(&task.reports, p) && !matches(&task.products, p))
    });
    let workspace = crate::document_sets::Workspace::load(root).map_err(|e| e.1)?;
    if result["status"] == "succeeded" && result["complete"] == true {
        for path in result["products"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if !safe_path(path) || protected(path) || !matches(&task.products, path)
                || !crate::document_sets::contained_relative_with_policy(root, path, |path| safe_path(path) && !protected(path))
                || workspace.sets.iter().any(|set| crate::document_sets::included(&set.docs, path))
            {
                return Ok(json!({"applied":false,"reason":format!("Product {path} cannot replace Documents, protected paths, or nested projects. Results remain in run history.")}));
            }
        }
    }
    let mut cases = 0;
    if !changed_input && !deleted_input {
        for set in &workspace.sets {
            let project = workspace.resolve(Some(&set.id)).map_err(|e| e.1)?;
            cases += import_evidence(
                &crate::store::Store::load(&project.out),
                &crate::gen::GenSettings::resolve(&project),
                task,
                result,
                id,
            );
        }
    }
    let mut products = Vec::new();
    if result["status"] == "succeeded" && result["complete"] == true {
        for path in result["products"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
        {
            if !safe_path(path) || protected(path) || !matches(&task.products, path) {
                continue;
            }
            let bytes = STANDARD
                .decode(
                    result["files"][path]
                        .as_str()
                        .ok_or("Missing product bytes")?,
                )
                .map_err(|_| "Invalid product bytes")?;
            let output = root.join(path);
            #[cfg(not(target_arch = "wasm32"))]
            if !crate::vfs::is_scoped()
                && output
                    .ancestors()
                    .take_while(|p| *p != root)
                    .any(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()))
            {
                return Err("Product destination contains a symbolic link.".into());
            }
            if let Some(parent) = output.parent() {
                crate::vfs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            crate::vfs::write(&output, bytes).map_err(|e| e.to_string())?;
            products.push(path.to_owned());
        }
    }
    Ok(
        json!({"applied":true,"criteria":cases,"products":products,"evidence_withheld":changed_input||deleted_input}),
    )
}

#[cfg(test)]
mod result_tests {
    use super::*;
    #[test]
    fn historical_and_retried_run_metadata_cannot_reload_saved_logs() {
        let log = "dependency installed\n".repeat(10000);
        let state = json!({"id":"saved-run","terminated":true,"log":log,
            "result":{"status":"succeeded","log":log},
            "summary":{"exit_code":0,"log":log}});
        let context = metadata_response(
            "task_context",
            json!({
                "available":true,"runs":[state.clone(),state.clone()],"next_cursor":"older-run"
            }),
        );
        assert!(context.to_string().len() < 1024);
        assert_eq!(context["runs"].as_array().unwrap().len(), 2);
        assert_eq!(context["next_cursor"], "older-run");
        for operation in ["run_task", "inspect_run", "wait_run", "cancel_run"] {
            let value = metadata_response(operation, state.clone());
            assert!(value.to_string().len() < 1024);
            assert_eq!(value["id"], "saved-run");
            assert_eq!(value["result"]["status"], "succeeded");
            assert_eq!(value["summary"]["exit_code"], 0);
        }
        let page = present(
            &state,
            &Value::Null,
            &json!({"operation":"read_run_log","limit":31}),
        )
        .unwrap();
        assert_eq!(
            metadata_response("read_run_log", page)["text"],
            log.chars().take(31).collect::<String>()
        );
        assert_eq!(state["log"], log);
    }
    #[test]
    fn status_polls_omit_logs_but_explicit_reads_preserve_them() {
        let log = "dependency installed\n".repeat(10000);
        for terminated in [false, true] {
            let state = json!({"id":"run-1","state":"succeeded","terminated":terminated,"log":log});
            let result = if terminated {
                json!({"status":"succeeded","exit_code":0,"complete":true,"log":log,"files":{},"tests":[]})
            } else {
                Value::Null
            };
            for operation in ["inspect_run", "wait_run"] {
                let status = present(&state, &result, &json!({"operation":operation})).unwrap();
                assert!(status.get("log").is_none());
                assert_eq!(status["terminated"], terminated);
                assert!(status.to_string().len() < 1024);
            }
            let page = present(
                &state,
                &result,
                &json!({"operation":"read_run_log","offset":3,"limit":17}),
            )
            .unwrap();
            assert_eq!(
                page["text"],
                log.chars().skip(3).take(17).collect::<String>()
            );
            assert_eq!(page["next_offset"], 20);
            assert_eq!(page["complete"], terminated);
            let matches = present(
                &state,
                &result,
                &json!({"operation":"search_run_log","query":"installed","limit":2}),
            )
            .unwrap();
            assert_eq!(matches["matches"].as_array().unwrap().len(), 2);
        }
    }
    fn task() -> Task {
        serde_json::from_value(json!({"id":"tests","name":"Tests","description":"Check","command":"mvn test","products":["dist/**"],"reports":["reports/*.xml"]})).unwrap()
    }
    fn result(xml: &str) -> Value {
        json!({"status":"succeeded","complete":true,"exit_code":0,"log":"done","files":{},"cache":{},"deleted":[],"products":[],"reports":{"reports/tests.xml":STANDARD.encode(xml)}})
    }
    #[test]
    fn case_evidence_is_conclusive_only_when_unique_and_executed() {
        let raw=result("<testsuite><testcase classname='A' name='yes'/><testcase classname='A' name='no'><failure/></testcase><testcase classname='A' name='skip'><skipped/></testcase></testsuite>");
        let parsed = validate_result(&task(), &BTreeMap::new(), raw).unwrap();
        assert_eq!(case_verdict(&parsed, "A::yes").0, "pass");
        assert_eq!(case_verdict(&parsed, "no").0, "fail");
        assert_eq!(case_verdict(&parsed, "skip").0, "none");
        assert_eq!(case_verdict(&parsed, "missing").0, "none");
        let parsed = validate_result(
            &task(),
            &BTreeMap::new(),
            result("<testsuite><testcase name='yes'/><testcase name='yes'/></testsuite>"),
        )
        .unwrap();
        assert_eq!(case_verdict(&parsed, "yes").0, "none");
        assert!(junit(b"<!DOCTYPE testsuite><testsuite/>").is_err());
        assert!(junit(b"<testsuite><testcase name='broken'>").is_err());
    }
    #[test]
    fn outputs_cannot_write_protected_or_unmatched_paths() {
        for path in [
            "../escape",
            ".jazyk/ds_main/status.yaml",
            "unmatched.txt",
            "dist/secret.key",
        ] {
            let mut raw = result("<testsuite/>");
            raw["files"][path] = json!(STANDARD.encode("forged"));
            assert!(
                validate_result(&task(), &BTreeMap::new(), raw).is_err(),
                "{path}"
            );
        }
    }
    #[test]
    fn snapshots_remove_credentials_and_stale_products_do_not_overwrite_edits() {
        let mut files = BTreeMap::new();
        files.insert(
            ".jazyk/jazyk.toml".into(),
            b"version=1\nsets=[]\n[llm]\napi_key='private-value'\n".to_vec(),
        );
        files.insert("source.txt".into(), b"before".to_vec());
        let _scope = crate::vfs::Scope::enter(&files);
        let root = Path::new("/project");
        let input = snapshot(root).unwrap();
        assert!(
            !String::from_utf8(STANDARD.decode(&input[".jazyk/jazyk.toml"]).unwrap())
                .unwrap()
                .contains("private-value")
        );
        let mut raw = result("<testsuite/>");
        raw["files"]["dist/slides.pdf"] = json!(STANDARD.encode(b"%PDF-1.7"));
        raw["products"] = json!(["dist/slides.pdf"]);
        let validated = validate_result(&task(), &input, raw).unwrap();
        crate::vfs::write(root.join("source.txt"), b"newer edit").unwrap();
        assert_eq!(
            apply_result(root, &task(), &input, &validated, "run-1").unwrap()["applied"],
            false
        );
        assert!(crate::vfs::read(root.join("dist/slides.pdf")).is_err());
        crate::vfs::write(root.join("source.txt"), b"before").unwrap();
        assert_eq!(
            apply_result(root, &task(), &input, &validated, "run-1").unwrap()["applied"],
            true
        );
        assert_eq!(
            crate::vfs::read(root.join("dist/slides.pdf")).unwrap(),
            b"%PDF-1.7"
        );
    }
}

pub fn task_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["id","name","description","command"],"properties":{
        "id":{"type":"string"},"name":{"type":"string"},"description":{"type":"string"},"command":{"type":"string"},
        "image":{"type":"string","description":"Linux OCI image with /bin/sh; defaults to python:3.12-slim"},"cwd":{"type":"string","description":"Project-relative directory; use . for the project root, never an absolute host path"},
        "size":{"type":"string","enum":["small","medium","large"]},"timeout_seconds":{"type":"integer","minimum":1,"maximum":7200},
        "env":{"type":"object","additionalProperties":{"type":"string"},"description":"Non-secret environment values"},
        "products":{"type":"array","items":{"type":"string"},"description":"Project-relative globs for generated files to retain"},
        "cache":{"type":"array","items":{"type":"string"},"description":"Disposable files restored before the command and saved only after success"},
        "reports":{"type":"array","items":{"type":"string"},"description":"Fresh JUnit XML output globs for per-case evidence; do not repeat a pattern in products or cache"}
    }})
}

#[cfg(not(target_arch = "wasm32"))]
pub fn cli(args: &[String]) -> Result<Value, String> {
    let root = project_root(&std::env::current_dir().map_err(|e| e.to_string())?)?;
    match args.first().map(String::as_str) {
        Some("tasks" | "runs") => call(&root, "task_context", &json!({})),
        Some("run") => {
            let id = args.get(1).ok_or("Usage: jazyk run <task-id>")?;
            let context = call(&root, "task_context", &json!({}))?;
            let key = format!(
                "cli-{}",
                &digest(format!("{}:{}", crate::sys::now_millis(), id).as_bytes())[..32]
            );
            call(
                &root,
                "run_task",
                &json!({"task_id":id,"expected_revision":context["revision"],"operation_key":key}),
            )
        }
        Some(command @ ("run-status" | "run-log" | "run-cancel")) => call(
            &root,
            match command {
                "run-status" => "inspect_run",
                "run-log" => "read_run_log",
                _ => "cancel_run",
            },
            &json!({"run_id":args.get(1).ok_or("A run ID is required")?}),
        ),
        _ => Err("Unknown task command".into()),
    }
}

#[cfg(test)]
mod retained_input_tests {
    use super::*;

    fn retained() -> Value {
        json!({"complete":true,
            "input_files":{"page.html":STANDARD.encode("<svg>original ž\r\n</svg>"),"changed.txt":STANDARD.encode("before"),"deleted.txt":STANDARD.encode("deleted input")},
            "files":{"changed.txt":STANDARD.encode("after"),"new.txt":STANDARD.encode("created")},
            "deleted":["deleted.txt"]})
    }

    #[test]
    fn retained_input_reads_unchanged_snapshot_without_claiming_a_changed_output() {
        let result = retained();
        let args = json!({"operation":"read_run_changes","path":"page.html"});
        let error = present(&Value::Null, &result, &args).unwrap_err();
        assert!(error.contains("frozen input exists") && error.contains("version: input"));
        let input = present(&Value::Null, &result, &json!({"operation":"read_run_changes","path":"page.html","version":"input"})).unwrap();
        assert_eq!(input, json!({"path":"page.html","version":"input","encoding":"utf8","content":"<svg>original ž\r\n</svg>"}));
        let listed = present(&Value::Null, &result, &json!({"operation":"read_run_changes"})).unwrap();
        assert_eq!(listed["files"].as_array().unwrap().len(), 3);
        assert!(listed["files"].as_array().unwrap().iter().all(|file| file["path"] != "page.html"));
        assert_eq!(present(&Value::Null, &result, &json!({"operation":"read_run_changes","version":"changes"})).unwrap(), listed);
    }

    #[test]
    fn retained_input_and_changed_versions_keep_their_own_bytes() {
        let result = retained();
        let changed = present(&Value::Null, &result, &json!({"operation":"read_run_changes","path":"changed.txt"})).unwrap();
        assert_eq!(changed["content"], "after");
        assert_eq!(changed["before"]["content"], "before");
        assert_eq!(changed["change"], "modified");
        let input = present(&Value::Null, &result, &json!({"operation":"read_run_changes","path":"changed.txt","version":"input"})).unwrap();
        assert_eq!(input["content"], "before");
        assert_eq!(input["version"], "input");
        assert!(input.get("change").is_none() && input.get("before").is_none());
        let deleted = present(&Value::Null, &result, &json!({"operation":"read_run_changes","path":"deleted.txt"})).unwrap();
        assert_eq!(deleted["encoding"], "absent");
        assert_eq!(deleted["change"], "deleted");
        let input = present(&Value::Null, &result, &json!({"operation":"read_run_changes","path":"deleted.txt","version":"input"})).unwrap();
        assert_eq!(input["content"], "deleted input");
        assert!(present(&Value::Null, &result, &json!({"operation":"read_run_changes","path":"new.txt","version":"input"})).unwrap_err().contains("Retained input file not found"));
    }

    #[test]
    fn retained_input_preserves_binary_downloads_and_bounds_previews() {
        for bytes in [vec![0u8, 255, 128, 13, 10], vec![b'x'; 65537]] {
            let encoded = STANDARD.encode(&bytes);
            let result = json!({"input_files":{"asset.bin":encoded}});
            let mut args = json!({"operation":"read_run_changes","path":"asset.bin","version":"input"});
            let preview = present(&Value::Null, &result, &args).unwrap();
            assert_eq!(preview["encoding"], "binary");
            assert_eq!(preview["size"], bytes.len());
            assert_eq!(preview["download"], true);
            args["download"] = json!("1");
            let full = present(&Value::Null, &result, &args).unwrap();
            assert_eq!(full["encoding"], "base64");
            assert_eq!(full["version"], "input");
            assert_eq!(STANDARD.decode(full["content"].as_str().unwrap()).unwrap(), bytes);
        }
    }

    #[test]
    fn retained_input_rejects_protected_paths_missing_inputs_and_invalid_modes() {
        let result = json!({"input_files":{".env":STANDARD.encode("private"),"src/private.key":STANDARD.encode("private"),"broken.txt":"not base64"}});
        for path in [".env", "../outside.txt", "src/private.key", ".git/config"] {
            let args = json!({"operation":"read_run_changes","path":path,"version":"input"});
            assert!(present(&Value::Null, &result, &args).unwrap_err().contains("Invalid output path"));
        }
        assert!(present(&Value::Null, &result, &json!({"operation":"read_run_changes","version":"input"})).unwrap_err().contains("requires path"));
        assert!(present(&Value::Null, &result, &json!({"operation":"read_run_changes","version":"current"})).unwrap_err().contains("version must be"));
        assert!(present(&Value::Null, &result, &json!({"operation":"read_run_changes","version":"input","path":"absent.txt"})).unwrap_err().contains("Retained input file not found"));
        assert!(present(&Value::Null, &result, &json!({"operation":"read_run_changes","version":"input","path":"broken.txt"})).unwrap_err().contains("Invalid retained input encoding"));
        assert!(present(&Value::Null, &result, &json!({"operation":"read_run_changes","path":"absent.txt"})).unwrap_err().contains("Omit path to list retained changes"));
    }
}

#[cfg(test)]
mod default_timeout_tests {
    use super::*;
    #[test]
    fn new_tasks_allow_cold_builds_and_keep_explicit_shorter_deadlines() {
        let mut value = serde_json::json!({"id":"build","name":"Build","description":"Build source","command":"cargo build --locked"});
        let task: Task = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(task.timeout_seconds, 1800);
        value["timeout_seconds"] = serde_json::json!(90);
        let task: Task = serde_json::from_value(value).unwrap();
        assert_eq!(task.timeout_seconds, 90);
    }
}
