//! Credentials for human ACP chat, shared by thin IDE clients, never project source.
use crate::llm::Llm;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
pub const ONBOARDING: &str =
    "Use /login for Jazyk Cloud or /setup for an OpenAI-compatible endpoint.";
#[derive(Deserialize, Serialize)]
struct Saved {
    source: String,
    base_url: String,
    model: String,
    api: String,
    api_key: String,
    #[serde(default)]
    expires_at: Option<u64>,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn path() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|p| p.join(".jazyk/chat-provider.json"))
        .ok_or("cannot locate home directory".into())
}
pub fn save(source: &str, llm: &Llm, expires_in: Option<u64>) -> Result<(), String> {
    let saved = Saved {
        source: source.into(),
        base_url: llm.base_url.clone(),
        model: llm.model.clone(),
        api: llm.api.clone(),
        api_key: llm.api_key.clone(),
        expires_at: expires_in.map(|n| now() + n),
    };
    save_at(&path()?, &saved)
}
fn save_at(path: &Path, saved: &Saved) -> Result<(), String> {
    let parent = path.parent().ok_or("missing credential parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let result = (|| {
        use std::io::Write;
        let mut file = opts
            .open(&tmp)
            .map_err(|e| format!("cannot create credential file: {e}"))?;
        file.write_all(&serde_json::to_vec(saved).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}
fn load_at(path: &Path, mut llm: Llm) -> Result<Llm, String> {
    let saved: Saved = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if saved.expires_at.is_some_and(|t| t <= now()) {
        return Err("Jazyk Cloud sign-in expired. Use /login again.".into());
    }
    llm.base_url = saved.base_url;
    llm.model = saved.model;
    llm.api = saved.api;
    llm.api_key = saved.api_key;
    llm.reasoning_summary = None;
    Ok(llm)
}
pub fn for_chat(llm: &Llm, cwd: &Path) -> Result<Llm, String> {
    let path = path()?;
    if path.exists() {
        return load_at(&path, llm.clone());
    }
    if configured(cwd) {
        Ok(llm.clone())
    } else {
        Err(ONBOARDING.into())
    }
}

pub fn configured(cwd: &Path) -> bool {
    // The proxy passes resolved defaults too. Preserve whether they came from a user.
    if let Ok(value) = std::env::var("JAZYK_CHAT_CONFIGURED") {
        return value == "true";
    }
    let root = crate::project::find_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
    let proj = crate::project::Project::load(&root);
    let global = crate::project::load_global_llm();
    std::env::var_os("JAZYK_LLM_BASE_URL").is_some()
        || std::env::var_os("JAZYK_MODEL").is_some()
        || proj.llm.base_url.is_some()
        || proj.llm.model.is_some()
        || global.base_url.is_some()
        || global.model.is_some()
}

pub fn begin(cwd: &Path, source: &str) -> String {
    let result = (|| -> Result<(), String> {
        let log = dirs::cache_dir()
            .ok_or("cannot locate cache")?
            .join("jazyk/chat-setup.log");
        std::fs::create_dir_all(log.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let out = options.open(log).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            out.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        std::process::Command::new(exe)
            .args(["gui", "--port", "0"])
            .env("JAZYK_CHAT_SETUP", source)
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .stdout(out.try_clone().map_err(|e| e.to_string())?)
            .stderr(out)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    match result {
        Ok(())=>format!("Opening Jazyk Web. {} Your choice is saved for IDE chat; return here and send your message again.",if source=="cloud" {"Sign in, open the agent chooser and choose Jazyk Cloud."}else{"Open the agent chooser and configure your OpenAI-compatible endpoint and model."}),
        Err(e)=>format!("Cannot open chat setup: {e}"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_are_private_and_expired_cloud_requires_login() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("provider.json");
        let mut llm = crate::cli::resolve_llm(
            &Default::default(),
            &Default::default(),
            &Default::default(),
            |_| None,
        );
        llm.base_url = "https://example.test/v1".into();
        llm.model = "my-model".into();
        llm.api_key = "private-key".into();
        let mut saved = Saved {
            source: "cloud".into(),
            base_url: llm.base_url.clone(),
            model: llm.model.clone(),
            api: llm.api.clone(),
            api_key: llm.api_key.clone(),
            expires_at: Some(now() + 3600),
        };
        save_at(&path, &saved).unwrap();
        assert_eq!(load_at(&path, llm.clone()).unwrap().api_key, "private-key");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        saved.expires_at = Some(0);
        save_at(&path, &saved).unwrap();
        assert!(load_at(&path, llm).err().unwrap().contains("/login"));
    }
}
