//! Authenticated project-scoped Cloud adapter for native MCP, ACP, and CLI clients.
use super::*;
use fs2::FileExt;
use std::{
    fs,
    io::{Read, Write},
    time::Duration,
};
fn endpoint() -> Result<String, String> {
    let value =
        std::env::var("JAZYK_RUNTIME_URL").map_err(|_| "No Cloud runtime endpoint configured")?;
    let url = url::Url::parse(&value).map_err(|_| "Invalid Cloud runtime endpoint")?;
    let loopback = url.host_str().is_some_and(|h| {
        h == "localhost" || h.parse::<std::net::IpAddr>().is_ok_and(|a| a.is_loopback())
    });
    if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Cloud runtime requires HTTPS without embedded credentials, query, or fragment.".into(),
        );
    }
    Ok(value.trim_end_matches('/').to_owned())
}
pub fn validate_host(root: &Path) -> Result<(), String> {
    endpoint()?;
    let path = std::env::var("JAZYK_RUNTIME_TOKEN_FILE")
        .map_err(|_| "Set JAZYK_RUNTIME_TOKEN_FILE to your private runtime token file.")?;
    let path = fs::canonicalize(path).map_err(|_| "Cannot read runtime token file.")?;
    let root = fs::canonicalize(root).map_err(|_| "Invalid project root.")?;
    if path.starts_with(root) {
        return Err(
            "Store the runtime token outside the project before submitting its files.".into(),
        );
    }
    Ok(())
}
fn ownership_lock(root: &Path) -> Result<fs::File, String> {
    let dir = root.join(".jazyk/runtime/remote_runs");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(".ownership"))
        .map_err(|e| e.to_string())?;
    file.lock_exclusive().map_err(|e| e.to_string())?;
    Ok(file)
}
fn send(url: &str, request: &Value) -> Result<Value, String> {
    let path = std::env::var("JAZYK_RUNTIME_TOKEN_FILE")
        .map_err(|_| "Set JAZYK_RUNTIME_TOKEN_FILE to your private runtime token file.")?;
    let meta = fs::metadata(&path).map_err(|_| "Cannot read runtime token file.")?;
    if !meta.is_file() || meta.len() > 1024 {
        return Err("Invalid runtime token file.".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err("Runtime token file must be private (chmod 600).".into());
        }
    }
    let token = fs::read_to_string(path).map_err(|_| "Cannot read runtime token file.")?;
    let token = token.trim();
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid runtime token.".into());
    }
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(45))
        .build();
    let response = agent
        .post(url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .send_string(&request.to_string());
    let response = match response {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            let value: Value =
                serde_json::from_reader(r.into_reader().take(8192)).unwrap_or(Value::Null);
            return Err(format!(
                "Cloud runtime {code}: {}",
                value["error"]["message"]
                    .as_str()
                    .unwrap_or("Request refused.")
            ));
        }
        Err(_) => {
            return Err(
                "Cloud runtime could not be reached. No fallback execution was attempted.".into(),
            )
        }
    };
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cloud runtime response was interrupted.")?;
    if bytes.len() > 128 * 1024 * 1024 {
        return Err("Cloud runtime response exceeds its limit.".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid Cloud runtime response.".into())
}
pub fn request(root: &Path, args: &Value) -> Result<Value, String> {
    validate_host(root)?;
    let url = endpoint()?;
    let operation = args["operation"].as_str().unwrap_or_default();
    let mut forwarded = args.clone();
    if let Some(map) = forwarded.as_object_mut() {
        map.remove("cancel_file");
    }
    let pending = if operation == "run_task" {
        let _lock = ownership_lock(root)?;
        if args["cancel_file"]
            .as_str()
            .is_some_and(|p| Path::new(p).exists())
        {
            return Err("The owning operation was cancelled.".into());
        }
        let key = args["operation_key"]
            .as_str()
            .filter(|s| valid_id(s))
            .ok_or("Invalid operation key")?;
        let path = root
            .join(".jazyk/runtime/remote_runs")
            .join(format!("pending-{key}.json"));
        if !path.exists() {
            fs::write(
                &path,
                json!({"url":url,"cancel_file":args["cancel_file"]}).to_string(),
            )
            .map_err(|e| e.to_string())?;
        }
        Some(path)
    } else {
        None
    };
    let value = send(&url, &forwarded)?;
    if operation == "run_task" {
        let id = value["id"]
            .as_str()
            .filter(|s| valid_id(s))
            .ok_or("Cloud run ID missing")?;
        let _lock = ownership_lock(root)?;
        let dir = root.join(".jazyk/runtime/remote_runs");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{id}.json"));
        if pending
            .as_ref()
            .is_some_and(|p| p.with_extension("cancel").exists())
            || args["cancel_file"]
                .as_str()
                .is_some_and(|p| Path::new(p).exists())
        {
            fs::write(path.with_extension("cancel"), b"cancelled").map_err(|e| e.to_string())?;
        }
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(json!({"url":url,"task":args["task"],"cancel_file":args["cancel_file"],"created_at":crate::sys::now_unix()}).to_string().as_bytes()).map_err(|e|e.to_string())?;
                let mut command =
                    std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
                command
                    .arg("__task_remote_watch")
                    .arg(root)
                    .arg(id)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    command.process_group(0);
                }
                if command.spawn().is_err() {
                    return Err(format!("Cloud run {id} was submitted, but its local observer could not start. Inspect that run to retrieve its result."));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                return Err(format!(
                    "Cloud run {id} was submitted but its receipt could not be saved: {e}"
                ))
            }
        }
    }
    if let Some(pending) = pending {
        let _lock = ownership_lock(root)?;
        let _ = fs::remove_file(pending.with_extension("cancel"));
        let _ = fs::remove_file(pending);
    }
    if matches!(operation, "inspect_run" | "wait_run") && value["terminated"] == true {
        if let Some(id) = args["run_id"].as_str().filter(|s| valid_id(s)) {
            let path = root
                .join(".jazyk/runtime/remote_runs")
                .join(format!("{id}.json"));
            if !path.exists() {
                return Ok(value);
            }
            let lock = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(path.with_extension("lock"))
                .map_err(|e| e.to_string())?;
            lock.lock_exclusive().map_err(|e| e.to_string())?;
            if let Ok(text) = fs::read_to_string(&path) {
                if let Ok(mut record) = serde_json::from_str::<Value>(&text) {
                    if record["url"] == url && record["imported"] != true {
                        let result = send(&url, &json!({"operation":"result","run_id":id}))?;
                        if !result["input_files"].is_object() {
                            return Ok(value);
                        }
                        let input: BTreeMap<String, String> =
                            serde_json::from_value(result["input_files"].clone())
                                .map_err(|_| "Cloud run inputs missing")?;
                        let task: Task = serde_json::from_value(record["task"].clone())
                            .map_err(|_| "Saved task missing")?;
                        record["import"] = apply_result(root, &task, &input, &result, id)?;
                        record["imported"] = json!(true);
                        fs::write(&path, record.to_string()).map_err(|e| e.to_string())?;
                    }
                }
            }
        }
    }
    Ok(value)
}
pub fn cancel_owned(root: &Path, fence: &Path) -> Result<(), String> {
    let dir = root.join(".jazyk/runtime/remote_runs");
    if !dir.exists() {
        return Ok(());
    }
    let _lock = ownership_lock(root)?;
    let url = endpoint()?;
    mark_owned(&dir, &url, fence)
}
fn mark_owned(dir: &Path, url: &str, fence: &Path) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        let record = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok());
        if record.is_some_and(|r| r["cancel_file"] == json!(fence) && r["url"] == url) {
            fs::write(path.with_extension("cancel"), b"cancelled").map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn watch(root: &Path, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err("Invalid remote run ID".into());
    }
    let path = root
        .join(".jazyk/runtime/remote_runs")
        .join(format!("{id}.json"));
    let record: Value =
        serde_json::from_str(&fs::read_to_string(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let url = endpoint()?;
    if record["url"] != url {
        return Err("Remote runtime endpoint changed.".into());
    }
    let deadline = record["created_at"]
        .as_u64()
        .unwrap_or(crate::sys::now_unix())
        + 8400;
    while crate::sys::now_unix() < deadline {
        if path.with_extension("cancel").exists()
            || record["cancel_file"]
                .as_str()
                .is_some_and(|p| Path::new(p).exists())
        {
            let _ = send(&url, &json!({"operation":"cancel_run","run_id":id}));
        }
        if let Ok(value) = request(root, &json!({"operation":"inspect_run","run_id":id})) {
            if value["terminated"] == true {
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err("Cloud run observer expired; inspect retained Cloud history.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_submissions_keep_cancellation_after_the_next_prompt() {
        let dir =
            std::env::temp_dir().join(format!("jazyk-remote-owner-{}", crate::sys::now_millis()));
        fs::create_dir_all(&dir).unwrap();
        let fence = dir.join("owner");
        let pending = dir.join("pending-request.json");
        fs::write(
            &pending,
            json!({"url":"https://example.invalid/runtime","cancel_file":fence}).to_string(),
        )
        .unwrap();
        fs::write(
            dir.join("independent.json"),
            json!({"url":"https://example.invalid/runtime","cancel_file":null}).to_string(),
        )
        .unwrap();
        fs::write(&fence, b"cancelled").unwrap();
        mark_owned(&dir, "https://example.invalid/runtime", &fence).unwrap();
        fs::remove_file(&fence).unwrap();
        assert!(pending.with_extension("cancel").exists());
        assert!(!dir.join("independent.cancel").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
