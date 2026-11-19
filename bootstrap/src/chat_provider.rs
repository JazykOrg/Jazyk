//! Credentials for human ACP chat, shared by thin IDE clients, never project source.
use crate::llm::Llm;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
pub const ONBOARDING: &str =
    "Use /jazyk login for Jazyk Cloud or /jazyk openai add NAME for an OpenAI-compatible endpoint.";
#[derive(Clone, Deserialize, Serialize)]
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
#[derive(Default, Deserialize, Serialize)]
struct Providers {
    selected: String,
    providers: std::collections::BTreeMap<String, Saved>,
}
fn providers_at(path: &Path) -> Result<Providers, String> {
    if !path.exists() {
        return Ok(Providers::default());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if let Ok(legacy) = serde_json::from_slice::<Saved>(&bytes) {
        return Ok(Providers {
            selected: legacy.source.clone(),
            providers: [(legacy.source.clone(), legacy)].into(),
        });
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
pub fn revision() -> Option<SystemTime> {
    std::fs::metadata(path().ok()?).ok()?.modified().ok()
}
pub fn selected() -> Option<String> {
    providers_at(&path().ok()?)
        .ok()
        .map(|p| p.selected)
        .filter(|p| !p.is_empty())
}
pub fn save(source: &str, llm: &Llm, expires_in: Option<u64>) -> Result<(), String> {
    let path = path()?;
    let mut providers = providers_at(&path)?;
    providers.selected = source.into();
    providers.providers.insert(
        source.into(),
        Saved {
            source: source.into(),
            base_url: llm.base_url.clone(),
            model: llm.model.clone(),
            api: llm.api.clone(),
            api_key: llm.api_key.clone(),
            expires_at: expires_in.map(|n| now() + n),
        },
    );
    save_at(&path, &providers)
}
fn save_at(path: &Path, saved: &impl Serialize) -> Result<(), String> {
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
fn apply(saved: Saved, mut llm: Llm) -> Result<Llm, String> {
    if saved.expires_at.is_some_and(|t| t <= now()) {
        return Err("Jazyk Cloud sign-in expired. Use /jazyk login again.".into());
    }
    llm.base_url = saved.base_url;
    llm.model = saved.model;
    llm.api = saved.api;
    llm.api_key = saved.api_key;
    llm.reasoning_summary = None;
    Ok(llm)
}
#[cfg(test)]
fn load_at(path: &Path, llm: Llm) -> Result<Llm, String> {
    let p = providers_at(path)?;
    apply(p.providers.get(&p.selected).ok_or(ONBOARDING)?.clone(), llm)
}
pub fn for_source(llm: &Llm, cwd: &Path, source: &str) -> Result<Llm, String> {
    for_source_configured(llm, source, configured(cwd))
}
pub fn for_source_configured(llm: &Llm, source: &str, configured: bool) -> Result<Llm, String> {
    let providers = providers_at(&path()?)?;
    if let Some(saved) = providers.providers.get(source) {
        return apply(saved.clone(), llm.clone());
    }
    if source == "endpoint" && configured {
        return Ok(llm.clone());
    }
    Err(if source == "cloud" {
        "Sign in to Jazyk Cloud with /login."
    } else {
        ONBOARDING
    }
    .into())
}
pub fn for_chat(llm: &Llm, cwd: &Path) -> Result<Llm, String> {
    for_source(llm, cwd, selected().as_deref().unwrap_or("endpoint"))
}
pub fn configured(_cwd: &Path) -> bool {
    // Defaults injected by the proxy must not opt human chat into compiler inference.
    if let Ok(value) = std::env::var("JAZYK_CHAT_CONFIGURED") {
        return value == "true";
    }
    std::env::var_os("JAZYK_LLM_BASE_URL").is_some()
}

pub fn entries() -> Vec<(String, String, String, bool)> {
    path()
        .ok()
        .and_then(|p| providers_at(&p).ok())
        .unwrap_or_default()
        .providers
        .into_iter()
        .map(|(id, p)| {
            (
                id,
                p.base_url,
                p.model,
                p.expires_at.is_none_or(|t| t > now()),
            )
        })
        .collect()
}
pub fn remove(source: &str) -> Result<(), String> {
    let path = path()?;
    let mut p = providers_at(&path)?;
    p.providers.remove(source);
    if p.selected == source {
        p.selected = "setup".into();
    }
    save_at(&path, &p)
}
pub fn begin(cwd: &Path, source: &str) -> String {
    let result = (|| -> Result<String, String> {
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
        let out = options.open(&log).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            out.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut command = std::process::Command::new(exe);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let ready = log.with_file_name(format!(
            "chat-setup-{}-{}.json",
            std::process::id(),
            crate::sys::now_millis()
        ));
        let mut child = command
            .args(["gui", "--no-open", "--port", "0"])
            .env(
                "JAZYK_CHAT_SETUP",
                if source == "cloud" {
                    "cloud-direct"
                } else {
                    source
                },
            )
            .env("JAZYK_CHAT_SETUP_READY", &ready)
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .stdout(out.try_clone().map_err(|e| e.to_string())?)
            .stderr(out)
            .spawn()
            .map_err(|e| e.to_string())?;
        for _ in 0..400 {
            if let Ok(bytes) = std::fs::read(&ready) {
                let url: String = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                let _ = std::fs::remove_file(&ready);
                open::that_detached(&url)
                    .map_err(|e| format!("cannot open browser ({e}); open {url}"))?;
                return Ok(url);
            }
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                return Err(format!(
                    "setup server exited ({status}); see the private chat-setup.log"
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = child.kill();
        Err("setup did not become ready within 20 seconds".into())
    })();
    match result {
        Ok(url) => format!(
            "Open setup: <{url}>\n{}",
            if source == "cloud" {
                "Sign in if needed, then authorize Jazyk. Return here when complete."
            } else {
                "Enter the endpoint details in the form. Return here after saving."
            }
        ),
        Err(e) => format!("Cannot open chat setup: {e}"),
    }
}
pub fn ready(url: &str) -> Result<(), String> {
    if let Some(path) = std::env::var_os("JAZYK_CHAT_SETUP_READY") {
        save_at(Path::new(&path), &url)?;
    }
    Ok(())
}
pub fn cloud_login() -> i32 {
    use sha2::{Digest, Sha256};
    let proof = match crate::gui::state::mint_token() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let challenge = format!("{:x}", Sha256::digest(proof.as_bytes()));
    let url = format!("https://jazyk.dev/account/cli-login?challenge={challenge}");
    if let Err(e) = ready(&url) {
        eprintln!("{e}");
        return 1;
    }
    let client = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build();
    for _ in 0..450 {
        match client
            .post("https://api.jazyk.dev/api/inference/exchange")
            .set("Content-Type", "application/json")
            .send_string(&serde_json::json!({"proof":proof}).to_string())
        {
            Ok(response) => {
                let grant: serde_json::Value = match response
                    .into_string()
                    .map_err(|e| e.to_string())
                    .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
                {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("invalid authorization reply: {e}");
                        return 1;
                    }
                };
                let Some(token) = grant["token"].as_str().filter(|s| s.len() == 64) else {
                    eprintln!("invalid Cloud grant");
                    return 1;
                };
                let mut llm = crate::cli::resolve_llm(
                    &Default::default(),
                    &Default::default(),
                    &Default::default(),
                    |_| None,
                );
                llm.base_url = "https://api.jazyk.dev/api/inference/v1".into();
                llm.model = "cloud".into();
                llm.api = "chat_completions".into();
                llm.api_key = token.into();
                return match save(
                    "cloud",
                    &llm,
                    Some(grant["expires_in"].as_u64().unwrap_or(3600).min(3600)),
                ) {
                    Ok(()) => 0,
                    Err(e) => {
                        eprintln!("{e}");
                        1
                    }
                };
            }
            Err(ureq::Error::Status(401, _)) => {}
            Err(e) => {
                eprintln!("Cloud authorization exchange failed: {e}");
                return 1;
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    eprintln!("Cloud authorization expired. Run /jazyk login again.");
    1
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
        assert!(load_at(&path, llm).err().unwrap().contains("/jazyk login"));
    }
}
