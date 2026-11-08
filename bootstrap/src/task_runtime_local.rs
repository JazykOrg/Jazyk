//! Local Docker/Podman supervisor. State is durable and independent of the MCP/ACP session.
use super::*;
use fs2::FileExt;
thread_local! { static ENGINE: std::cell::RefCell<String> = std::cell::RefCell::new("docker".into()); }
fn select_engine(value: &str) -> Result<(), String> {
    if !matches!(value, "docker" | "podman") {
        return Err("JAZYK_CONTAINER_ENGINE must be docker or podman.".into());
    }
    ENGINE.with(|e| *e.borrow_mut() = value.to_owned());
    Ok(())
}
fn engine_name() -> String {
    ENGINE.with(|e| e.borrow().clone())
}
fn run_lock(dir: &Path) -> Result<fs::File, String> {
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("finalization.lock"))
        .map_err(|e| e.to_string())?;
    file.lock_exclusive().map_err(|e| e.to_string())?;
    Ok(file)
}

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
                include_str!("../../runtime-worker/go.sum"),
                include_str!("../../runtime-worker/go.mod")
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
    let temp = path.with_extension(format!(
        "{}.{}.tmp",
        crate::sys::process_id(),
        crate::sys::now_millis()
    ));
    fs::write(&temp, serde_json::to_vec(value).unwrap()).map_err(|e| e.to_string())?;
    fs::rename(temp, path).map_err(|e| e.to_string())
}
fn run_dir(root: &Path, id: &str) -> Result<std::path::PathBuf, String> {
    if !valid_id(id) {
        return Err("Invalid run ID.".into());
    }
    Ok(root.join(".jazyk/runtime/runs").join(id))
}
fn cancelled(dir: &Path) -> bool {
    dir.join("cancel").exists()
        || read(&dir.join("request.json"))
            .ok()
            .and_then(|r| r["cancel_file"].as_str().map(std::path::PathBuf::from))
            .is_some_and(|p| p.exists())
}
pub fn cancel_owned(root: &Path, fence: &Path) -> Result<(), String> {
    if let Some(parent) = fence.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(fence, b"cancelled").map_err(|e| e.to_string())?;
    let runs = root.join(".jazyk/runtime/runs");
    if !runs.exists() {
        return Ok(());
    }
    let admission = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(runs.join(".admission"))
        .map_err(|e| e.to_string())?;
    admission.lock_exclusive().map_err(|e| e.to_string())?;
    for entry in fs::read_dir(&runs).map_err(|e| e.to_string())?.flatten() {
        let dir = entry.path();
        if read(&dir.join("request.json"))
            .ok()
            .is_some_and(|r| r["cancel_file"] == json!(fence))
        {
            let _lock = run_lock(&dir)?;
            if read(&dir.join("state.json"))
                .ok()
                .is_some_and(|r| r["finalized"] != true && r["terminated"] != true)
            {
                fs::write(dir.join("cancel"), b"cancelled").map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}
pub fn request(root: &Path, args: &Value) -> Result<Value, String> {
    let operation = args["operation"].as_str().unwrap_or_default();
    if matches!(operation, "task_context" | "run_task") {
        select_engine(
            &std::env::var("JAZYK_CONTAINER_ENGINE").unwrap_or_else(|_| "docker".into()),
        )?;
    }
    if operation == "task_context" {
        let mut runs = Vec::new();
        if let Ok(entries) = fs::read_dir(root.join(".jazyk/runtime/runs")) {
            for entry in entries.flatten() {
                if let Ok(run) = read(&entry.path().join("state.json")) {
                    runs.push(run)
                }
            }
        }
        runs.sort_by_key(|r| {
            std::cmp::Reverse((
                r["created_at"].as_u64().unwrap_or(0),
                r["id"].as_str().unwrap_or("").to_owned(),
            ))
        });
        if let Some(before) = args["before"].as_str() {
            let position = runs
                .iter()
                .position(|r| r["id"] == before)
                .ok_or("Invalid run cursor")?;
            runs.drain(..=position);
        }
        let next = if runs.len() > 30 {
            runs[29]["id"].clone()
        } else {
            Value::Null
        };
        runs.truncate(30);
        let readiness = docker(&["info", "--format", "json"]);
        return Ok(
            json!({"available":readiness.is_ok(),"reason":readiness.err(),"location":"local","provider":engine_name(),"runs":runs,"next_cursor":next}),
        );
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
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.parent().unwrap().join(".admission"))
            .map_err(|e| e.to_string())?;
        lock.lock_exclusive().map_err(|e| e.to_string())?;
        if args["cancel_file"]
            .as_str()
            .is_some_and(|p| Path::new(p).exists())
        {
            return Err("The owning operation was cancelled.".into());
        }
        if dir.exists() {
            let old = read(&dir.join("request.json"))?;
            if old["input_digest"] != args["input_digest"] || old["task"] != args["task"] {
                return Err("Operation key already refers to a different run.".into());
            }
            return read(&dir.join("state.json"));
        }
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
        let state = json!({"id":id,"task":args["task"],"input_digest":args["input_digest"],"state":"queued","terminated":false,"created_at":crate::sys::now_unix(),"location":"local","provider":engine_name()});
        write(&dir.join("state.json"), &state)?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut watcher = Command::new(&exe);
        watcher
            .arg("__task_watchdog")
            .arg(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            watcher.process_group(0);
        }
        if let Err(error) = watcher.spawn() {
            let mut failed = state.clone();
            failed["state"] = json!("failed");
            failed["terminated"] = json!(true);
            write(&dir.join("state.json"), &failed)?;
            return Err(format!("Cannot start deadline watchdog: {error}"));
        }
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
    select_engine(state["provider"].as_str().unwrap_or("docker"))?;
    if operation == "cancel_run" {
        let _lock = run_lock(&dir)?;
        state = read(&dir.join("state.json"))?;
        if state["terminated"] != true && state["finalized"] != true {
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
    if matches!(operation, "read_run_log" | "search_run_log") && state["terminated"] != true {
        let token = digest(dir.to_string_lossy().as_bytes());
        state["log"] =
            json!(docker(&["logs", &format!("jazyk-run-{}", &token[..20])]).unwrap_or_default());
    }
    let mut result = if state["terminated"] == true {
        read(&dir.join("validated.json")).unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    if operation == "result" {
        let mut result = result;
        result["input_files"] = read(&dir.join("request.json"))?["files"].clone();
        return Ok(result);
    }
    if operation == "read_run_changes" && !result.is_null() {
        result["input_files"] = read(&dir.join("request.json"))?["files"].clone();
    }
    super::present(&state, &result, args)
}
thread_local! { static SUPERVISOR: std::cell::RefCell<Option<(std::path::PathBuf,Instant)>> = const { std::cell::RefCell::new(None) }; }
fn docker(args: &[&str]) -> Result<String, String> {
    let cleanup = matches!(args.first(), Some(&"stop" | &"rm" | &"inspect"))
        || args.starts_with(&["volume", "rm"]);
    let mut command = Command::new(engine_name());
    let owned;
    let args = if engine_name() == "podman" && args.first() == Some(&"cp") {
        owned = [vec!["cp", "--archive=false"], args[1..].to_vec()].concat();
        owned.as_slice()
    } else {
        args
    };
    let mut child = command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{} is unavailable: {e}", engine_name()))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || read_capped(stdout, 65536));
    let err = std::thread::spawn(move || read_capped(stderr, 65536));
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        let cancelled = !cleanup
            && SUPERVISOR.with(|v| {
                v.borrow()
                    .as_ref()
                    .is_some_and(|(p, t)| cancelled(p) || t.elapsed() > Duration::from_secs(600))
            });
        if cancelled
            || start.elapsed()
                > Duration::from_secs(if args.first() == Some(&"info") {
                    3
                } else if cleanup {
                    20
                } else {
                    300
                })
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if cancelled {
                "Task setup cancelled."
            } else {
                "Container engine operation timed out."
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
// The first `cap` bytes of a stream, read to its end: the engine keeps writing past
// the cap into a pipe that is still drained, so a long answer never fails the
// command with a broken pipe or blocks it on a full one.
fn read_capped(mut pipe: impl std::io::Read, cap: usize) -> Vec<u8> {
    let mut kept = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = cap.saturating_sub(kept.len());
                kept.extend_from_slice(&chunk[..n.min(room)]);
            }
        }
    }
    kept
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
pub fn watchdog(dir: &Path) -> Result<(), String> {
    let initial = read(&dir.join("state.json"))?;
    select_engine(initial["provider"].as_str().unwrap_or("docker"))?;
    let timeout = initial["task"]["timeout_seconds"].as_u64().unwrap_or(300);
    let deadline = initial["created_at"].as_u64().unwrap_or(0) + timeout + 720;
    loop {
        let state = read(&dir.join("state.json"))?;
        if state["terminated"] == true {
            return Ok(());
        }
        if crate::sys::now_unix() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    let token = digest(dir.to_string_lossy().as_bytes());
    let name = format!("jazyk-run-{}", &token[..20]);
    loop {
        let _ = docker(&["rm", "-f", &name]);
        let _ = docker(&["rm", "-f", &format!("{name}-init")]);
        let _ = docker(&["volume", "rm", &format!("{name}-helper")]);
        let gone = docker(&["inspect", "--format", "{{.State.Running}}", &name])
            .map(|v| v == "false")
            .unwrap_or_else(|e| {
                e.to_ascii_lowercase().contains("no such")
                    || e.to_ascii_lowercase().contains("not found")
            });
        if gone {
            let mut state = read(&dir.join("state.json"))?;
            if state["terminated"] != true {
                state["state"] = json!(if cancelled(dir) {
                    "cancelled"
                } else {
                    "timed_out"
                });
                state["terminated"] = json!(true);
                state["error"] = json!("The independent deadline watchdog stopped this run.");
                write(&dir.join("state.json"), &state)?;
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(10));
    }
}
pub fn supervise(dir: &Path) -> Result<(), String> {
    SUPERVISOR.with(|s| *s.borrow_mut() = Some((dir.to_path_buf(), Instant::now())));
    let request = read(&dir.join("request.json"))?;
    let task: Task = serde_json::from_value(request["task"].clone()).map_err(|e| e.to_string())?;
    let mut state = read(&dir.join("state.json"))?;
    select_engine(state["provider"].as_str().unwrap_or("docker"))?;
    let token = digest(dir.to_string_lossy().as_bytes());
    let name = format!("jazyk-run-{}", &token[..20]);
    let volume = format!("{name}-helper");
    let init = format!("{name}-init");
    state["state"] = json!("provisioning");
    write(&dir.join("state.json"), &state)?;
    let mut allocation_attempted = false;
    let outcome = (|| -> Result<(), String> {
        if cancelled(dir) {
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
        let (cpus, ram) = task.resources();
        docker(&[
            "create",
            "--name",
            &name,
            "--user",
            "0",
            "--workdir",
            "/project",
            "--cpus",
            &cpus.to_string(),
            "--memory",
            &format!("{ram}g"),
            "--log-driver",
            "json-file",
            "--log-opt",
            "max-size=3m",
            "--pids-limit",
            "512",
            "--security-opt",
            "no-new-privileges",
            "--cap-drop",
            "ALL",
            "--mount",
            &format!("type=volume,source={volume},target=/runtime,readonly"),
            "-e",
            "JAZYK_INPUT=/tmp/input.json",
            "-e",
            "JAZYK_OUTPUT=/tmp/result.json",
            "-e",
            "SSL_CERT_FILE=/runtime/ca.crt",
            "--entrypoint",
            "/runtime/runner",
            &task.image,
        ])?;
        if cancelled(dir) {
            return Err("Cancelled before command start.".into());
        }
        let workspace = dir.join("io/project");
        fs::create_dir_all(&workspace).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&workspace, fs::Permissions::from_mode(0o777))
                .map_err(|e| e.to_string())?;
        }
        docker(&[
            "cp",
            workspace.to_str().ok_or("Invalid workspace path")?,
            &format!("{name}:/project"),
        ])?;
        docker(&[
            "cp",
            dir.join("io/input.json")
                .to_str()
                .ok_or("Invalid input path")?,
            &format!("{name}:/tmp/input.json"),
        ])?;
        docker(&["start", &name])?;
        state["state"] = json!("executing");
        write(&dir.join("state.json"), &state)?;
        let start = Instant::now();
        loop {
            if cancelled(dir) || start.elapsed() > Duration::from_secs(task.timeout_seconds + 120) {
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
        state["log"] = json!(docker(&["logs", &name]).unwrap_or_default());
        docker(&[
            "cp",
            &format!("{name}:/tmp/result.json"),
            dir.join("io/result.json")
                .to_str()
                .ok_or("Invalid result path")?,
        ])?;
        let _finalization = run_lock(dir)?;
        let files: BTreeMap<String, String> =
            serde_json::from_value(request["files"].clone()).map_err(|e| e.to_string())?;
        let mut result = super::validate_result(&task, &files, read(&dir.join("io/result.json"))?)?;
        if cancelled(dir) {
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
        state["state"] = if cancelled(dir) {
            json!("cancelled")
        } else {
            result["status"].clone()
        };
        if state["state"] == "succeeded"
            && result["complete"] == true
            && result["cache_skipped"] != true
        {
            fs::create_dir_all(cache_path.parent().unwrap()).map_err(|e| e.to_string())?;
            write(&cache_path, &result["cache"])?;
        }
        state["finalized"] = json!(true);
        write(&dir.join("state.json"), &state)?;
        Ok(())
    })();
    // Cleanup is unconditional, including partial setup. Inspect confirms no live allocation.
    let _ = docker(&["rm", "-f", &name]);
    let _ = docker(&["rm", "-f", &init]);
    let _ = docker(&["volume", "rm", &volume]);
    let terminated = !allocation_attempted
        || docker(&["inspect", "--format", "{{.State.Running}}", &name])
            .map(|v| v == "false")
            .unwrap_or_else(|e| {
                e.to_ascii_lowercase().contains("no such")
                    || e.to_ascii_lowercase().contains("not found")
            });
    if let Err(e) = outcome {
        state["state"] = json!(if cancelled(dir) {
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

#[cfg(test)]
mod tests {
    use super::*;

    // A fake container engine: a shell script at an absolute path, selected for this
    // test thread only (the engine choice is thread-local). No real engine runs.
    #[cfg(unix)]
    fn fake_engine(tag: &str, script: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "jazyk-fake-engine-{}-{}-{}",
            tag,
            crate::sys::process_id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("engine");
        fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        ENGINE.with(|e| *e.borrow_mut() = path.to_string_lossy().into_owned());
        dir
    }

    #[cfg(unix)]
    fn reset_engine(dir: &std::path::Path) {
        ENGINE.with(|e| *e.borrow_mut() = "docker".into());
        SUPERVISOR.with(|s| *s.borrow_mut() = None);
        fs::remove_dir_all(dir).ok();
    }

    // An engine answering far past 64 KB on both streams finishes with its own status:
    // the answer keeps its first 64 KB, and a failure reports its stderr, capped.
    #[cfg(unix)]
    #[test]
    fn engine_output_past_the_cap_is_drained_and_kept_to_its_head() {
        let dir = fake_engine(
            "drain",
            "if [ \"$1\" = fail ]; then head -c 300000 /dev/zero | tr '\\0' e 1>&2; exit 3; fi\nhead -c 200000 /dev/zero | tr '\\0' a; head -c 200000 /dev/zero 1>&2",
        );
        let out = docker(&["image", "ls"]).unwrap();
        assert_eq!(out.len(), 65536);
        assert!(out.bytes().all(|b| b == b'a'));
        let err = docker(&["fail"]).unwrap_err();
        assert_eq!(err.chars().count(), 2000);
        assert!(err.chars().all(|c| c == 'e'));
        reset_engine(&dir);
    }

    // A probe that hangs times out and the engine is killed; a missing engine is
    // reported unavailable.
    #[cfg(unix)]
    #[test]
    fn a_hanging_engine_probe_times_out() {
        let dir = fake_engine("timeout", "exec sleep 60");
        assert_eq!(
            docker(&["info", "--format", "json"]).unwrap_err(),
            "Container engine operation timed out."
        );
        ENGINE.with(|e| *e.borrow_mut() = dir.join("missing").to_string_lossy().into_owned());
        let err = docker(&["info"]).unwrap_err();
        assert!(err.contains("is unavailable"), "{err}");
        reset_engine(&dir);
    }

    // A supervised run's cancel file stops a running engine command, and so does a
    // supervisor past its lifetime; cleanup commands still run to the end.
    #[cfg(unix)]
    #[test]
    fn cancellation_stops_engine_commands_but_not_cleanup() {
        let dir = fake_engine(
            "cancel",
            "case \"$1\" in stop|rm|inspect) echo cleaned;; *) exec sleep 60;; esac",
        );
        let run = dir.join("run");
        fs::create_dir_all(&run).unwrap();
        fs::write(run.join("cancel"), b"cancelled").unwrap();
        SUPERVISOR.with(|s| *s.borrow_mut() = Some((run.clone(), Instant::now())));
        assert_eq!(
            docker(&["run", "--rm", "helper"]).unwrap_err(),
            "Task setup cancelled."
        );
        assert_eq!(docker(&["stop", "c1"]).unwrap(), "cleaned");
        fs::remove_file(run.join("cancel")).unwrap();
        if let Some(old) = Instant::now().checked_sub(Duration::from_secs(601)) {
            SUPERVISOR.with(|s| *s.borrow_mut() = Some((run.clone(), old)));
            assert_eq!(
                docker(&["cp", "a", "b"]).unwrap_err(),
                "Task setup cancelled."
            );
        }
        reset_engine(&dir);
    }

    // A child writing past the cap keeps its pipe drained: it finishes with its own
    // status instead of dying on a broken pipe, and the reader keeps the head.
    #[cfg(unix)]
    #[test]
    fn a_long_engine_answer_is_drained_past_the_cap() {
        use std::process::{Command, Stdio};
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("head -c 1000000 /dev/zero || exit 3; echo done 1>&2")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let out = read_capped(child.stdout.take().unwrap(), 65536);
        let status = child.wait().unwrap();
        assert!(status.success());
        assert_eq!(out.len(), 65536);
        assert!(out.iter().all(|b| *b == 0));
        let mut err = String::new();
        std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
        assert_eq!(err.trim(), "done");
    }

    #[test]
    fn existing_run_control_survives_invalid_task_settings() {
        let root = std::env::temp_dir().join(format!(
            "jazyk-runtime-invalid-{}",
            crate::sys::now_millis()
        ));
        let dir = root.join(".jazyk/runtime/runs/existing");
        fs::create_dir_all(&dir).unwrap();
        fs::write(root.join(".jazyk/jazyk.toml"), "[[tasks]]\ninvalid = [").unwrap();
        write(
            &dir.join("state.json"),
            &json!({"id":"existing","state":"executing","terminated":false,"provider":"docker"}),
        )
        .unwrap();
        assert!(super::super::load(&root).is_err());
        let stopped =
            super::super::call(&root, "cancel_run", &json!({"run_id":"existing"})).unwrap();
        assert_eq!(stopped["state"], "cancelling");
        assert!(dir.join("cancel").exists());
        assert!(super::super::call(&root, "inspect_run", &json!({"run_id":"existing"})).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn engine_selection_is_fixed_and_not_an_executable_path() {
        assert!(select_engine("podman").is_ok());
        assert_eq!(engine_name(), "podman");
        assert!(select_engine("sh -c evil").is_err());
        assert!(select_engine("/tmp/docker").is_err());
        select_engine("docker").unwrap();
    }
    #[test]
    fn cancellation_only_targets_owned_unfinalized_runs() {
        let root = std::env::temp_dir().join(format!(
            "jazyk-runtime-owner-{}-{}",
            crate::sys::process_id(),
            crate::sys::now_millis()
        ));
        let fence = root.join(".jazyk/owner");
        for (id, owned, finalized) in [
            ("child", true, false),
            ("independent", false, false),
            ("finished", true, true),
        ] {
            let dir = run_dir(&root, id).unwrap();
            fs::create_dir_all(&dir).unwrap();
            write(
                &dir.join("request.json"),
                &json!({"cancel_file":if owned {json!(fence)}else{Value::Null}}),
            )
            .unwrap();
            write(
                &dir.join("state.json"),
                &json!({"finalized":finalized,"terminated":false}),
            )
            .unwrap();
        }
        cancel_owned(&root, &fence).unwrap();
        assert!(run_dir(&root, "child").unwrap().join("cancel").exists());
        assert!(!run_dir(&root, "independent")
            .unwrap()
            .join("cancel")
            .exists());
        assert!(!run_dir(&root, "finished").unwrap().join("cancel").exists());
        fs::remove_file(&fence).unwrap();
        assert!(cancelled(&run_dir(&root, "child").unwrap()));
        fs::remove_dir_all(root).unwrap();
    }
}
