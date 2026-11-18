//! Paseo is an ACP client. Provider discovery never submits a model prompt.
use serde_json::json;
use std::path::{Path, PathBuf};

pub fn config_path() -> PathBuf {
    std::env::var_os("PASEO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".paseo"))
        .join("config.json")
}

fn register(path: &Path, command: &str) -> Result<(), String> {
    for (name, value) in [
        ("extends", json!("acp")),
        ("label", json!("Jazyk")),
        ("command", json!([command, "acp", "--agent", "embedded"])),
    ] {
        crate::editor_lsp::merge_nested(path, "agents.providers.jazyk", name, &value)?;
    }
    Ok(())
}

pub fn configure() -> Result<(), String> {
    let command = std::env::current_exe().map_err(|e| e.to_string())?;
    register(&config_path(), &command.to_string_lossy())?;
    for action in ["start", "reload"] {
        let status = std::process::Command::new("paseo")
            .arg(action)
            .env_remove("PASEO_HOST")
            .status()
            .map_err(|e| format!("cannot run paseo {action}: {e}"))?;
        if !status.success() {
            return Err(format!(
                "paseo {action} exited {status}; check the local Paseo daemon"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_is_idempotent_and_keeps_other_providers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"agents":{"providers":{"claude":{"enabled":true},"jazyk":{"options":{"custom":true}}}},"port":6767}"#,
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
        assert_eq!(
            doc["agents"]["providers"]["jazyk"]["options"]["custom"],
            true
        );
        register(&path, "/a path/jazyk").unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }
}
