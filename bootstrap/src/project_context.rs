//! Read-only integration metadata shared by MCP initialize, chat, and ACP.
use serde_json::{json, Value};
use std::path::Path;

pub fn read(root: &Path) -> Value {
    match crate::task_runtime::integration_context(root) {
        Ok(value) => value,
        Err(error) => json!({"available":false,"error":error}),
    }
}

// The integration facts as initialize instructions, framed as metadata.
// Mirrors docs/frontends/mcp.md#project-integration-context.
pub const INSTRUCTIONS: &str =
    include_str!("../../docs/compiler/goals/prompts/integration-context.md");

pub fn instructions(root: &Path) -> String {
    let text = crate::payload::fill(
        INSTRUCTIONS.trim_end_matches('\n'),
        &[("context", &read(root).to_string())],
    );
    format!("\n{}\n", text)
}

pub fn local(root: &Path) -> Result<Value, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = root;
        Ok(json!({"available":true,"location":"browser","github":null}))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Git interprets its own standard metadata, including linked worktrees. A
        // read never runs a repository's configured monitor, never takes optional
        // locks, and gives up at its limit (docs/frontends/mcp.md#project-integration-context).
        let git = |args: &[&str]| -> Option<String> {
            let mut command = std::process::Command::new("git");
            command
                .env("GIT_OPTIONAL_LOCKS", "0")
                .args(["-c", "core.fsmonitor=false", "-C"])
                .arg(root)
                .args(args);
            output_within(command, GIT_LIMIT)
        };
        let branch = git(&["symbolic-ref", "--quiet", "--short", "HEAD"]);
        let remote = git(&["config", "--get", "remote.origin.url"]);
        let repository = remote.as_deref().and_then(github_url);
        let default_branch = git(&[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ])
        .and_then(|s| s.strip_prefix("origin/").map(str::to_owned));
        let changed = git(&["status", "--porcelain", "--untracked-files=normal"])
            .map(|s| s.lines().filter(|s| !s.is_empty()).count());
        Ok(
            json!({"available":true,"location":"local","github":repository.map(|url| json!({
                "url":url,"branch":branch,"default_branch":default_branch,
                "default_branch_source":"local origin/HEAD, not a fresh remote check",
                "base_commit":git(&["rev-parse","--verify","HEAD"]),"local_changes":changed,
                "actions":[]
            }))}),
        )
    }
}

#[cfg(not(target_arch = "wasm32"))]
const GIT_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

// A successful command's trimmed stdout, or None on failure or past the limit.
#[cfg(not(target_arch = "wasm32"))]
fn output_within(mut command: std::process::Command, limit: std::time::Duration) -> Option<String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        if started.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let bytes = reader.join().ok()?.ok()?;
    status
        .success()
        .then(|| String::from_utf8_lossy(&bytes).trim().to_owned())
}

// Publish only a canonical GitHub URL. Never echo remote credentials or query data.
#[cfg(not(target_arch = "wasm32"))]
fn github_url(remote: &str) -> Option<String> {
    let path = if let Some(path) = remote.strip_prefix("git@github.com:") {
        path.to_owned()
    } else {
        let url = url::Url::parse(remote).ok()?;
        if url.host_str()? != "github.com" || !matches!(url.scheme(), "https" | "ssh") {
            return None;
        }
        url.path().trim_start_matches('/').to_owned()
    };
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|s| {
            s.is_empty()
                || *s == "."
                || *s == ".."
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    {
        return None;
    }
    Some(format!("https://github.com/{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn local_context_distinguishes_selected_and_default_branches() {
        let dir = tempfile::tempdir().unwrap();
        for args in [
            vec!["init", "--initial-branch=main"],
            vec![
                "remote",
                "add",
                "origin",
                "git@github.com:JazykOrg/jazyk-site.git",
            ],
            vec![
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/master",
            ],
        ] {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .unwrap()
                .status
                .success());
        }
        let value = local(dir.path()).unwrap();
        assert_eq!(
            value["github"]["url"],
            "https://github.com/JazykOrg/jazyk-site"
        );
        assert_eq!(value["github"]["branch"], "main");
        assert_eq!(value["github"]["default_branch"], "master");
        assert!(value["github"]["base_commit"].is_null());
    }

    #[test]
    #[cfg(unix)]
    fn local_context_never_runs_a_configured_fsmonitor() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("monitor-ran");
        let hook = dir.path().join("monitor.sh");
        std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        for args in [
            vec!["init".to_string()],
            vec![
                "config".into(),
                "core.fsmonitor".into(),
                hook.display().to_string(),
            ],
        ] {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(&args)
                .output()
                .unwrap()
                .status
                .success());
        }
        local(dir.path()).unwrap();
        assert!(
            !marker.exists(),
            "a metadata read executed the repository's fsmonitor"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_command_past_its_limit_yields_nothing() {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "sleep 5; echo late"]);
        let started = std::time::Instant::now();
        assert_eq!(
            output_within(command, std::time::Duration::from_millis(100)),
            None
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "echo ok"]);
        assert_eq!(
            output_within(command, std::time::Duration::from_secs(5)).as_deref(),
            Some("ok")
        );
    }

    #[test]
    fn github_urls_drop_credentials_and_reject_non_repositories() {
        assert_eq!(
            github_url("git@github.com:JazykOrg/jazyk-site.git").as_deref(),
            Some("https://github.com/JazykOrg/jazyk-site")
        );
        assert_eq!(
            github_url("https://secret:token@github.com/owner/repo.git?token=private").as_deref(),
            Some("https://github.com/owner/repo")
        );
        for url in [
            "https://example.com/owner/repo",
            "https://github.com/owner/repo/tree/main",
            "https://github.com/owner/%72epo",
        ] {
            assert!(github_url(url).is_none());
        }
    }
}
