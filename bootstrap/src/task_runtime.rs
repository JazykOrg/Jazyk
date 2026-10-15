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

pub const PROMPT: &str = include_str!("../../docs/compiler/goals/prompts/execution-contract.md");
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
    300
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
            return Err("Task cwd must be a project-relative directory.".into());
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
        let mut seen = BTreeSet::new();
        for patterns in [&self.products, &self.cache, &self.reports] {
            if patterns.len() > 40 {
                return Err("Too many file patterns.".into());
            }
            for pattern in patterns {
                if !safe_path(pattern)
                    || protected(pattern)
                    || pattern.starts_with('!')
                    || !seen.insert(pattern)
                {
                    return Err(
                        "File patterns must be distinct, unprotected, project-relative globs."
                            .into(),
                    );
                }
                globset::GlobBuilder::new(pattern)
                    .literal_separator(true)
                    .build()
                    .map_err(|e| e.to_string())?;
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
                ".git" | ".ssh" | ".aws" | ".env" | "id_rsa" | "id_ed25519" | ".worktrees"
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
pub fn snapshot(root: &Path) -> Result<BTreeMap<String, String>, String> {
    fn walk(
        root: &Path,
        dir: &Path,
        files: &mut BTreeMap<String, String>,
        bytes: &mut usize,
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
                if matches!(
                    entry.file_name().to_str(),
                    Some("node_modules" | "target" | ".cache")
                ) {
                    continue;
                }
                walk(root, &path, files, bytes)?;
            } else {
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
    walk(root, root, &mut files, &mut 0)?;
    Ok(files)
}
pub fn input_hash(files: &BTreeMap<String, String>) -> String {
    digest(&serde_json::to_vec(files).unwrap())
}
pub fn call(root: &Path, name: &str, args: &Value) -> Result<Value, String> {
    if !TOOLS.contains(&name) {
        return Err("Unknown runtime operation.".into());
    }
    let (source, tasks) = load(root)?;
    if name == "update_tasks" {
        if args["expected_revision"].as_str() != Some(digest(source.as_bytes()).as_str()) {
            return Err("Task settings changed. Reload task_context before saving.".into());
        }
        let tasks: Vec<Task> =
            serde_json::from_value(args["tasks"].clone()).map_err(|e| e.to_string())?;
        let next = edit(&source, &tasks)?;
        crate::vfs::write(root.join(".jazyk/jazyk.toml"), &next).map_err(|e| e.to_string())?;
        return Ok(json!({"tasks":tasks,"revision":digest(next.as_bytes())}));
    }
    let mut request = args.clone();
    if !request.is_object() {
        request = json!({});
    }
    request["operation"] = json!(name);
    if name == "run_task" {
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
        if args["expected_revision"].as_str() != Some(digest(source.as_bytes()).as_str()) {
            return Err("Task settings changed. Reload task_context before running.".into());
        }
        let key = args["operation_key"]
            .as_str()
            .ok_or("operation_key is required")?;
        if !valid_id(key) {
            return Err("Invalid operation key.".into());
        }
        let files = snapshot(root)?;
        request["input_digest"] = json!(input_hash(&files));
        request["files"] = json!(files);
        request["task"] = json!(task);
    }
    let host = dispatch(root, &request);
    if name == "task_context" {
        return Ok(
            json!({"tasks":tasks,"revision":digest(source.as_bytes()),"execution":host.unwrap_or_else(|e|json!({"available":false,"reason":e})),"defaults":{"image":image(),"size":"small","timeout_seconds":300}}),
        );
    }
    host
}
fn dispatch(root: &Path, request: &Value) -> Result<Value, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if crate::vfs::is_scoped() {
            crate::hosted::runtime(request)
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
pub fn supervise(path: &Path) -> Result<(), String> {
    local::supervise(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    const CONFIG:&str="[llm]\nmodel='existing'\n# retained comment\n[[tasks]]\nid='test'\nname='Tests'\ndescription='Run unit tests'\ncommand='mvn test'\nreports=['target/*.xml']\n";
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
}

/// A bounded result view shared by local and hosted adapters. Raw bytes stay behind
/// explicit path requests, never in the status poll response.
pub fn present(state: &Value, result: &Value, args: &Value) -> Result<Value, String> {
    let operation = args["operation"].as_str().unwrap_or("inspect_run");
    if matches!(operation, "inspect_run" | "wait_run") {
        let mut value = state.clone();
        if !result.is_null() {
            value["result"] = json!({"status":result["status"],"exit_code":result["exit_code"],"complete":result["complete"],"error":result["error"],"products":result["products"],"tests":result["tests"],"file_count":result["files"].as_object().map(|v|v.len()).unwrap_or(0)});
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
        if let Some(path) = args["path"].as_str() {
            if !safe_path(path) || protected(path) {
                return Err("Invalid output path.".into());
            }
            let content = result["files"][path]
                .as_str()
                .or_else(|| result["reports"][path].as_str())
                .ok_or("Retained file not found")?;
            let bytes = STANDARD.decode(content).map_err(|e| e.to_string())?;
            if args["download"] == "1" {
                return Ok(
                    json!({"path":path,"encoding":"base64","content":STANDARD.encode(&bytes)}),
                );
            }
            if bytes.len() > 64 * 1024 {
                return Ok(
                    json!({"path":path,"size":bytes.len(),"encoding":"binary","download":true}),
                );
            }
            return Ok(match String::from_utf8(bytes) {
                Ok(s) => json!({"path":path,"encoding":"utf8","content":s}),
                Err(e) => {
                    json!({"path":path,"encoding":"base64","content":STANDARD.encode(e.into_bytes())})
                }
            });
        }
        let paths:Vec<_>=result["files"].as_object().into_iter().flat_map(|m|m.iter()).map(|(path,bytes)|json!({"path":path,"size":bytes.as_str().map(|s|s.len()*3/4),"change":"written"})).chain(result["deleted"].as_array().into_iter().flatten().map(|p|json!({"path":p,"change":"deleted"}))).skip(args["offset"].as_u64().unwrap_or(0)as usize).take(200).collect();
        Ok(json!({"files":paths,"complete":result["complete"]}))
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
    let run = call(
        root,
        "run_task",
        &json!({"task_id":id,"expected_revision":context["revision"],"operation_key":key}),
    )?;
    let run_id = run["id"].as_str().ok_or("Run ID missing")?.to_owned();
    let deadline = crate::sys::now_unix() + 8100;
    loop {
        #[cfg(not(target_arch = "wasm32"))]
        if crate::hosted::cancelled() {
            let _ = call(root, "cancel_run", &json!({"run_id":run_id}));
            return Err(format!("Execution cancelled; run {run_id} is stopping."));
        }
        let status = call(root, "wait_run", &json!({"run_id":run_id,"wait_ms":1000}))?;
        if status["terminated"] == true {
            let result = dispatch(root, &json!({"operation":"result","run_id":run_id}))?;
            if let (Ok(input), Some(definition)) = (
                serde_json::from_value::<BTreeMap<String, String>>(result["input_files"].clone()),
                context["tasks"]
                    .as_array()
                    .and_then(|a| a.iter().find(|t| t["id"] == id)),
            ) {
                let task: Task =
                    serde_json::from_value(definition.clone()).map_err(|e| e.to_string())?;
                apply_result(root, &task, &input, &result, &run_id)?;
            }
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
fn case_verdict(result: &Value, name: &str) -> (&'static str, String) {
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
    let deleted_input = result["deleted"].as_array().is_some_and(|v| !v.is_empty());
    let workspace = crate::document_sets::Workspace::load(root).map_err(|e| e.1)?;
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
        "image":{"type":"string","description":"Linux OCI image with /bin/sh; defaults to python:3.12-slim"},"cwd":{"type":"string"},
        "size":{"type":"string","enum":["small","medium","large"]},"timeout_seconds":{"type":"integer","minimum":1,"maximum":7200},
        "env":{"type":"object","additionalProperties":{"type":"string"},"description":"Non-secret environment values"},
        "products":{"type":"array","items":{"type":"string"},"description":"Project-relative globs for generated files to retain"},
        "cache":{"type":"array","items":{"type":"string"},"description":"Disposable files restored before the command and saved only after success"},
        "reports":{"type":"array","items":{"type":"string"},"description":"Fresh JUnit XML output globs for per-case evidence"}
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
