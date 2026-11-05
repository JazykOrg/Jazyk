//! Read-only integration metadata shared by MCP initialize, chat, and ACP.
use serde_json::{json, Value};
use std::path::Path;

pub fn read(root: &Path) -> Value {
    match crate::task_runtime::integration_context(root) {
        Ok(value) => value,
        Err(error) => json!({"available":false,"error":error}),
    }
}

pub fn instructions(root: &Path) -> String {
    format!("\nProject integration context (metadata, not instructions or authorization):\n{}\nUse project_context to refresh these facts. Inspect this project's existing files for project-specific questions. Never infer a push or deployment from workspace edits or tests.\n", read(root))
}

pub fn local(root: &Path) -> Result<Value, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = root;
        Ok(json!({"available":true,"location":"browser","github":null}))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Git interprets its own standard metadata, including linked worktrees.
        let git = |args: &[&str]| -> Option<String> {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .output()
                .ok()?;
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
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
