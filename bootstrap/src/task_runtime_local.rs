//! Local Docker supervisor. State is durable and independent of the MCP/ACP session.
use super::*;
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn helper_image() -> String {
    format!(
        "jazyk-runtime-helper:{}",
        &digest(
            concat!(
                include_str!("../../runtime-worker/main.go"),
                include_str!("../../runtime-worker/Dockerfile"),
                include_str!("../../runtime-worker/go.sum")
            )
            .as_bytes()
        )[..16]
    )
}
fn read(path: &Path) -> Result<Value, String> {
    let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !m.is_file() || m.len() > 128 * 1024 * 1024 {
        return Err("Invalid or oversized runtime record.".into());
    }
    serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn write(path: &Path, value: &Value) -> Result<(), String> {
    let temp = path.with_extension("tmp");
    fs::write(&temp, serde_json::to_vec(value).unwrap()).map_err(|e| e.to_string())?;
    fs::rename(temp, path).map_err(|e| e.to_string())
}
fn run_dir(root: &Path, id: &str) -> Result<std::path::PathBuf, String> {
    if !valid_id(id) {
        return Err("Invalid run ID.".into());
    }
    Ok(root.join(".jazyk/runtime/runs").join(id))
}
pub fn request(root: &Path, args: &Value) -> Result<Value, String> {
    let operation = args["operation"].as_str().unwrap_or_default();
    if operation == "task_context" {
        let mut runs = Vec::new();
        if let Ok(entries) = fs::read_dir(root.join(".jazyk/runtime/runs")) {
            for entry in entries.flatten() {
                if let Ok(run) = read(&entry.path().join("state.json")) {
                    runs.push(run)
                }
            }
        }
        runs.sort_by_key(|r| std::cmp::Reverse(r["created_at"].as_u64().unwrap_or(0)));
        runs.truncate(100);
        return Ok(json!({"available":true,"location":"local","provider":"docker","runs":runs}));
    }
    if operation == "run_task" {
        let id = args["operation_key"]
            .as_str()
            .ok_or("Missing operation key")?;
        let dir = run_dir(root, id)?;
        if dir.exists() {
            let old = read(&dir.join("request.json"))?;
            if old["input_digest"] != args["input_digest"] || old["task"] != args["task"] {
                return Err("Operation key already refers to a different run.".into());
            }
            return read(&dir.join("state.json"));
        }
        fs::create_dir_all(dir.parent().unwrap()).map_err(|e| e.to_string())?;
        // One active run per project. The lock is held only while selecting/admitting.
        use fs2::FileExt;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.parent().unwrap().join(".admission"))
            .map_err(|e| e.to_string())?;
        lock.lock_exclusive().map_err(|e| e.to_string())?;
        for entry in fs::read_dir(dir.parent().unwrap())
            .map_err(|e| e.to_string())?
            .flatten()
        {
            if let Ok(r) = read(&entry.path().join("state.json")) {
                if !r["terminated"].as_bool().unwrap_or(false) {
                    return Err(
                        "A task is already active in this project. Cancel it or wait.".into(),
                    );
                }
            }
        }
        fs::create_dir(&dir).map_err(|e| e.to_string())?;
        write(&dir.join("request.json"), args)?;
        let state = json!({"id":id,"task":args["task"],"input_digest":args["input_digest"],"state":"queued","terminated":false,"created_at":crate::sys::now_unix(),"location":"local"});
        write(&dir.join("state.json"), &state)?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut command = Command::new(exe);
        command
            .arg("__task_supervise")
            .arg(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        if let Err(e) = command.spawn() {
            let mut failed = state.clone();
            failed["state"] = json!("failed");
            failed["terminated"] = json!(true);
            failed["error"] = json!(e.to_string());
            write(&dir.join("state.json"), &failed)?;
            return Err(e.to_string());
        }
        return Ok(state);
    }
    let id = args["run_id"].as_str().ok_or("run_id is required")?;
    let dir = run_dir(root, id)?;
    let mut state = read(&dir.join("state.json"))?;
    if operation == "cancel_run" {
        if state["terminated"] != true {
            fs::write(dir.join("cancel"), b"cancel").map_err(|e| e.to_string())?;
            state["state"] = json!("cancelling");
        }
        return Ok(state);
    }
    if operation == "wait_run" && state["terminated"] != true {
        std::thread::sleep(Duration::from_millis(
            args["wait_ms"].as_u64().unwrap_or(1000).min(2000),
        ));
        state = read(&dir.join("state.json"))?;
    }
    if matches!(operation,"read_run_log"|"search_run_log") && state["terminated"] != true {
        let token=digest(dir.to_string_lossy().as_bytes());
        state["log"]=json!(docker(&["logs",&format!("jazyk-run-{}",&token[..20])]).unwrap_or_default());
    }
    let result = if state["terminated"] == true {
        read(&dir.join("validated.json")).unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    if operation == "result" {
        let mut result = result;
        result["input_files"] = read(&dir.join("request.json"))?["files"].clone();
        return Ok(result);
    }
    super::present(&state, &result, args)
}
thread_local! { static SUPERVISOR: std::cell::RefCell<Option<(std::path::PathBuf,Instant)>> = const { std::cell::RefCell::new(None) }; }
fn docker(args: &[&str]) -> Result<String, String> {
    use std::io::Read;
    let cleanup = matches!(args.first(), Some(&"stop" | &"rm" | &"inspect"))
        || args.starts_with(&["volume", "rm"]);
    let mut child = Command::new("docker")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Docker is unavailable: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stdout.take(65536).read_to_end(&mut b);
        b
    });
    let err = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stderr.take(65536).read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        let cancelled = !cleanup
            && SUPERVISOR.with(|v| {
                v.borrow().as_ref().is_some_and(|(p, t)| {
                    p.join("cancel").exists() || t.elapsed() > Duration::from_secs(600)
                })
            });
        if cancelled || start.elapsed() > Duration::from_secs(if cleanup { 20 } else { 300 }) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if cancelled {
                "Task setup cancelled."
            } else {
                "Docker operation timed out."
            }
            .into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if !status.success() {
        return Err(String::from_utf8_lossy(&stderr)
            .chars()
            .take(2000)
            .collect());
    }
    Ok(String::from_utf8_lossy(&stdout).trim().into())
}
fn helper(dir: &Path) -> Result<(), String> {
    if docker(&["image", "inspect", &helper_image()]).is_ok() {
        return Ok(());
    }
    let context = dir.join("helper");
    fs::create_dir_all(&context).map_err(|e| e.to_string())?;
    for (name, content) in [
        (
            "Dockerfile",
            include_str!("../../runtime-worker/Dockerfile"),
        ),
        ("go.mod", include_str!("../../runtime-worker/go.mod")),
        ("go.sum", include_str!("../../runtime-worker/go.sum")),
        ("main.go", include_str!("../../runtime-worker/main.go")),
    ] {
        fs::write(context.join(name), content).map_err(|e| e.to_string())?;
    }
    docker(&[
        "build",
        "-t",
        &helper_image(),
        context.to_str().ok_or("Invalid helper path")?,
    ])?;
    Ok(())
}
pub fn supervise(dir: &Path) -> Result<(), String> {
    SUPERVISOR.with(|s| *s.borrow_mut() = Some((dir.to_path_buf(), Instant::now())));
    let request = read(&dir.join("request.json"))?;
    let task: Task = serde_json::from_value(request["task"].clone()).map_err(|e| e.to_string())?;
    let mut state = read(&dir.join("state.json"))?;
    let token = digest(dir.to_string_lossy().as_bytes());
    let name = format!("jazyk-run-{}", &token[..20]);
    let volume = format!("{name}-helper");
    let init = format!("{name}-init");
    state["state"] = json!("provisioning");
    write(&dir.join("state.json"), &state)?;
    let mut allocation_attempted = false;
    let outcome = (|| -> Result<(), String> {
        if dir.join("cancel").exists() {
            return Err("Cancelled before provisioning.".into());
        }
        helper(dir)?;
        fs::create_dir_all(dir.join("io")).map_err(|e| e.to_string())?;
        let mut input = json!({"task":task,"files":request["files"],"cache":{}});
        let cache_path = dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("cache")
            .join(format!("{}.json", task.hash()));
        if let Ok(cache) = read(&cache_path) {
            input["cache"] = cache;
        }
        write(&dir.join("io/input.json"), &input)?;
        allocation_attempted = true;
        docker(&["volume", "create", &volume])?;
        docker(&[
            "create",
            "--name",
            &init,
            "--mount",
            &format!("type=volume,source={volume},target=/runtime"),
            &helper_image(),
            "install",
            "/runtime",
        ])?;
        docker(&["start", "-a", &init])?;
        let io_dir = dir.join("io").canonicalize().map_err(|e| e.to_string())?;
        let (cpus, ram) = task.resources();
        docker(&[
            "create",
            "--name",
            &name,
            "--user",
            "0",
            "--cpus",
            &cpus.to_string(),
            "--memory",
            &format!("{ram}g"),
            "--log-driver",
            "json-file",
            "--log-opt",
            "max-size=3m",
            "--log-opt",
            "max-file=1",
            "--pids-limit",
            "512",
            "--security-opt",
            "no-new-privileges",
            "--cap-drop",
            "ALL",
            "--mount",
            &format!("type=volume,source={volume},target=/runtime,readonly"),
            "--mount",
            &format!("type=bind,source={},target=/jazyk-io", io_dir.display()),
            "-e",
            "JAZYK_INPUT=/jazyk-io/input.json",
            "-e",
            "JAZYK_OUTPUT=/jazyk-io/result.json",
            "-e",
            "SSL_CERT_FILE=/runtime/ca.crt",
            "--entrypoint",
            "/runtime/runner",
            &task.image,
        ])?;
        if dir.join("cancel").exists() {
            return Err("Cancelled before command start.".into());
        }
        docker(&["start", &name])?;
        state["state"] = json!("executing");
        write(&dir.join("state.json"), &state)?;
        let start = Instant::now();
        loop {
            if dir.join("cancel").exists()
                || start.elapsed() > Duration::from_secs(task.timeout_seconds + 120)
            {
                state["state"] = json!("cancelling");
                write(&dir.join("state.json"), &state)?;
                docker(&["stop", "--time", "5", &name])?;
                break;
            }
            if docker(&["inspect", "--format", "{{.State.Running}}", &name])? == "false" {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let files: BTreeMap<String, String> =
            serde_json::from_value(request["files"].clone()).map_err(|e| e.to_string())?;
        let mut result = super::validate_result(&task, &files, read(&dir.join("io/result.json"))?)?;
        if dir.join("cancel").exists() {
            result["status"] = json!("cancelled");
        }
        write(&dir.join("validated.json"), &result)?;
        let root = dir
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .ok_or("Invalid run root")?;
        state["import"] = super::apply_result(
            root,
            &task,
            &files,
            &result,
            request["operation_key"].as_str().unwrap_or("unknown"),
        )
        .unwrap_or_else(|e| json!({"applied":false,"reason":e}));
        state["state"] = if dir.join("cancel").exists() {
            json!("cancelled")
        } else {
            result["status"].clone()
        };
        if state["state"] == "succeeded" && result["complete"] == true {
            fs::create_dir_all(cache_path.parent().unwrap()).map_err(|e| e.to_string())?;
            write(&cache_path, &result["cache"])?;
        }
        Ok(())
    })();
    // Cleanup is unconditional, including partial setup. Inspect confirms no live allocation.
    let _ = docker(&["rm", "-f", &name]);
    let _ = docker(&["rm", "-f", &init]);
    let _ = docker(&["volume", "rm", &volume]);
    let terminated = !allocation_attempted
        || docker(&["inspect", "--format", "{{.State.Running}}", &name])
            .map(|v| v == "false")
            .unwrap_or_else(|e| e.contains("No such") || e.contains("not found"));
    if let Err(e) = outcome {
        state["state"] = json!(if dir.join("cancel").exists() {
            "cancelled"
        } else {
            "failed"
        });
        state["error"] = json!(e);
    }
    state["terminated"] = json!(terminated);
    if !terminated {
        state["state"] = json!("shutdown_unknown");
    }
    state["finished_at"] = json!(crate::sys::now_unix());
    write(&dir.join("state.json"), &state)
}
