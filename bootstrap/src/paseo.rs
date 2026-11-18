//! Paseo is an ACP client. Provider discovery never submits a model prompt.
use serde_json::json;
use std::path::{Path, PathBuf};

pub fn home() -> PathBuf {
    std::env::var_os("PASEO_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".paseo"))
}
pub fn config_path_at(home: &Path) -> PathBuf {
    home.join("config.json")
}
pub fn config_path() -> PathBuf {
    config_path_at(&home())
}

fn register(path: &Path, command: &str) -> Result<(), String> {
    for (name, value) in [
        ("extends", json!("acp")),
        ("label", json!("Jazyk")),
        ("enabled", json!(true)),
        ("command", json!([command, "acp", "--agent", "embedded"])),
    ] {
        crate::editor_lsp::merge_nested(path, "agents.providers.jazyk", name, &value)?;
    }
    Ok(())
}

fn reload_args_for(config: &Path) -> Vec<String> {
    vec![
        "--home".into(),
        config.parent().unwrap().display().to_string(),
        "reload".into(),
    ]
}
pub fn reload_args() -> Vec<String> {
    reload_args_for(&config_path())
}

pub fn configure() -> Result<(), String> {
    configure_at(&home())
}
pub fn configure_at(home: &Path) -> Result<(), String> {
    let path = config_path_at(home);
    let command = std::env::current_exe().map_err(|e| e.to_string())?;
    register(&path, &command.to_string_lossy())?;
    let args = reload_args_for(&path);
    let status = std::process::Command::new("paseo")
        .args(&args)
        .status()
        .map_err(|e| format!("cannot reload existing Paseo daemon: {e}"))?;
    if !status.success() {
        return Err(format!("cannot reload existing Paseo daemon for {} ({status}); connect that server before retrying",home.display()));
    }
    let output = std::process::Command::new("paseo")
        .arg("--home")
        .arg(home)
        .args(["provider", "ls", "--json"])
        .output()
        .map_err(|e| format!("cannot verify Paseo provider registration: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cannot read providers from Paseo server at {} ({})",
            home.display(),
            output.status
        ));
    }
    let providers: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).map_err(|e| {
            format!(
                "invalid provider list from Paseo server at {}: {e}",
                home.display()
            )
        })?;
    if !providers
        .iter()
        .any(|row| row["provider"] == "jazyk" && row["enabled"] == "Enabled")
    {
        return Err(format!("Paseo at {} reloaded but did not register enabled Jazyk; check the selected server and provider overrides",home.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reload_targets_only_the_selected_existing_daemon() {
        assert_eq!(
            reload_args_for(Path::new("/chosen home/config.json")),
            ["--home", "/chosen home", "reload"]
        );
    }
    #[test]
    fn registration_is_idempotent_and_keeps_other_providers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"agents":{"providers":{"claude":{"enabled":true},"jazyk":{"options":{"custom":true},"enabled":false}}},"port":6767}"#,
        )
        .unwrap();
        register(&path, "/a path/jazyk").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            doc["agents"]["providers"]["jazyk"]["command"],
            json!(["/a path/jazyk", "acp", "--agent", "embedded"])
        );
        assert_eq!(doc["agents"]["providers"]["claude"]["enabled"], true);
        assert_eq!(doc["port"], 6767);
        assert_eq!(doc["agents"]["providers"]["jazyk"]["enabled"], true);
        assert_eq!(
            doc["agents"]["providers"]["jazyk"]["options"]["custom"],
            true
        );
        register(&path, "/a path/jazyk").unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }
}
