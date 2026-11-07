// Editor adapters launch the shared LSP. Mirrors docs/frontends/lsp.md#editor-setup.
use crate::acp::install::{merge_into, Merge};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn executable() -> Result<String, String> {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

fn artifact(relative: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .map(|p| p.join("editors").join(relative))
        .chain(std::iter::once(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("editors")
                .join(relative),
        ))
        .find(|p| p.is_file())
}

fn read_jsonc(path: &Path) -> Result<Value, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!({})),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let value = jsonc_parser::parse_to_serde_value::<Option<Value>>(&text, &Default::default())
        .map_err(|e| format!("{}: {e}", path.display()))?
        .unwrap_or_else(|| json!({}));
    if !value.is_object() {
        return Err(format!("{}: expected a settings object", path.display()));
    }
    Ok(value)
}

// Splice only the managed fields. Other servers, nested keys and comments survive.
fn zed_settings(path: &Path, command: &str) -> Result<bool, String> {
    let value = read_jsonc(path)?;
    for pointer in [
        "/languages",
        "/languages/Markdown",
        "/lsp",
        "/lsp/jazyk",
        "/lsp/jazyk/binary",
    ] {
        if let Some(value) = value.pointer(pointer) {
            if !value.is_object() {
                return Err(format!("{}: {pointer} must be an object", path.display()));
            }
        }
    }
    let current = &value["languages"]["Markdown"]["language_servers"];
    let mut servers: Vec<String> = if current.is_null() {
        vec!["...".into()]
    } else {
        serde_json::from_value(current.clone()).map_err(|_| {
            format!(
                "{}: Markdown.language_servers must be a string list",
                path.display()
            )
        })?
    };
    servers.retain(|s| s != "jazyk" && s != "!jazyk");
    servers.insert(0, "jazyk".into());
    // Merge the nested object by splicing at the leaf, rather than serializing
    // Markdown or jazyk (which may contain hand-written comments).
    let mut changed = false;
    for (key, name, entry) in [
        ("languages.Markdown", "language_servers", json!(servers)),
        ("lsp.jazyk.binary", "path", json!(command)),
        ("lsp.jazyk.binary", "arguments", json!(["lsp"])),
    ] {
        changed |= merge_nested(path, key, name, &entry)?;
    }
    Ok(changed)
}

fn merge_nested(path: &Path, key: &str, name: &str, entry: &Value) -> Result<bool, String> {
    // merge_into supports dotted property names literally, so descend with the
    // JSONC AST and splice each managed leaf into its containing object.
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let updated = nested_splice(&text, &key.split('.').collect::<Vec<_>>(), name, entry)?;
    let Some(updated) = updated else {
        return Ok(false);
    };
    write_atomic(path, updated.as_bytes())?;
    Ok(true)
}

fn nested_splice(
    text: &str,
    keys: &[&str],
    name: &str,
    entry: &Value,
) -> Result<Option<String>, String> {
    use jsonc_parser::ast::Value as JValue;
    use jsonc_parser::common::Ranged;
    use jsonc_parser::{parse_to_ast, CollectOptions, ParseOptions};
    let ast = parse_to_ast(text, &CollectOptions::default(), &ParseOptions::default())
        .map_err(|e| e.to_string())?;
    let root = ast.value;
    let mut value = root.as_ref();
    let mut parent = None;
    let mut missing = keys.len();
    for (i, key) in keys.iter().enumerate() {
        match value {
            Some(JValue::Object(object)) => {
                parent = Some(object);
                if let Some(prop) = object.properties.iter().find(|p| p.name.as_str() == *key) {
                    value = Some(&prop.value);
                } else {
                    missing = i;
                    break;
                }
            }
            None => {
                missing = i;
                break;
            }
            _ => return Err(format!("{key}: expected an object")),
        }
    }
    let mut leaf = entry.clone();
    let target_name;
    if missing < keys.len() {
        leaf = json!({name: leaf});
        for key in keys[missing + 1..].iter().rev() {
            leaf = json!({*key: leaf});
        }
        target_name = keys[missing];
    } else {
        target_name = name;
        match value {
            Some(JValue::Object(object)) => parent = Some(object),
            None => {}
            _ => return Err(format!("{}: expected an object", keys.join("."))),
        }
    }
    if let Some(object) = parent {
        if let Some(prop) = object
            .properties
            .iter()
            .find(|p| p.name.as_str() == target_name)
        {
            let range = prop.value.range();
            let previous: Value = jsonc_parser::parse_to_serde_value::<Value>(
                &text[range.start..range.end],
                &Default::default(),
            )
            .map_err(|e| e.to_string())?;
            if previous == leaf {
                return Ok(None);
            }
            let mut out = text.to_string();
            out.replace_range(
                range.start..range.end,
                &serde_json::to_string(&leaf).unwrap(),
            );
            return Ok(Some(out));
        }
        // Insert immediately after the opening brace. Existing properties and their
        // comments retain their exact bytes, including any trailing comma.
        let insert = object.range.start + 1;
        let comma = if object.properties.is_empty() {
            ""
        } else {
            ","
        };
        let mut out = text.to_string();
        out.insert_str(
            insert,
            &format!("\n  {}: {}{}", json!(target_name), leaf, comma),
        );
        return Ok(Some(out));
    }
    Ok(Some(format!("{text}\n{}\n", json!({target_name: leaf}))))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<bool, String> {
    let resolved;
    let path = if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        resolved = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        &resolved
    } else {
        path
    };
    if std::fs::read(path).ok().as_deref() == Some(bytes) {
        return Ok(false);
    }
    let parent = path.parent().ok_or("missing parent directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    if let Ok(metadata) = std::fs::metadata(path) {
        std::fs::set_permissions(&tmp, metadata.permissions()).map_err(|e| e.to_string())?;
    }
    if let Err(error) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {error}", path.display()));
    }
    Ok(true)
}

fn zed_data(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Zed")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Local"))
            .join("Zed")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("zed")
    }
}

pub fn zed(root: &Path) -> Result<(), String> {
    let settings = root.join(".zed/settings.json");
    if zed_settings(&settings, &executable()?)? {
        println!("jazyk: configured Zed LSP ({})", settings.display());
    }
    let Some(wasm) = artifact("zed/extension.wasm") else {
        eprintln!("jazyk: Zed LSP needs the built adapter; see bootstrap/editors/zed/README.md (or install the Jazyk dev extension)");
        return Ok(());
    };
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or("cannot locate home directory")?;
    let install = zed_data(Path::new(&home)).join("extensions/installed/jazyk");
    let manifest = include_bytes!("../editors/zed/extension.toml");
    // A dev extension is owned by Zed's dev workflow. Do not write through its link.
    if std::fs::symlink_metadata(&install).is_ok_and(|m| m.file_type().is_symlink()) {
        println!(
            "jazyk: using Zed Jazyk dev extension; rebuild it through Zed after adapter changes"
        );
        return Ok(());
    }
    let mut changed = write_atomic(
        &install.join("extension.wasm"),
        &std::fs::read(wasm).map_err(|e| e.to_string())?,
    )?;
    changed |= write_atomic(&install.join("extension.toml"), manifest)?;
    if changed {
        println!("jazyk: installed Zed LSP adapter; reload extensions if Zed has not refreshed");
    }
    Ok(())
}

// product-info.json identifies the exact profile used by this installation.
fn product_info(program: &Path, args: &[String]) -> Option<(PathBuf, Value)> {
    let app = args
        .windows(2)
        .find(|w| w[0] == "-na")
        .map(|w| PathBuf::from(&w[1]));
    let resolved = std::fs::canonicalize(program).unwrap_or_else(|_| program.to_path_buf());
    let bases = app
        .into_iter()
        .chain(resolved.ancestors().map(Path::to_path_buf));
    for base in bases {
        for relative in [
            "Contents/Resources/product-info.json",
            "Resources/product-info.json",
            "product-info.json",
        ] {
            let path = base.join(relative);
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(value) = serde_json::from_slice(&bytes) {
                    return Some((path, value));
                }
            }
        }
    }
    None
}

fn jetbrains_plugins(home: &Path, profile: &str) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/JetBrains")
            .join(profile)
            .join("plugins")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Roaming"))
            .join("JetBrains")
            .join(profile)
            .join("plugins")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("JetBrains")
            .join(profile)
    }
}

pub fn jetbrains(root: &Path, launch: Option<(PathBuf, Vec<String>)>) -> Result<(), String> {
    let config = root.join(".jazyk/ide.json");
    read_jsonc(&config)?;
    if matches!(
        merge_into(&config, "lsp", "command", &json!(executable()?))?,
        Merge::Written
    ) {
        println!("jazyk: configured JetBrains LSP ({})", config.display());
    }
    let Some(jar) = artifact("jetbrains/build/libs/jazyk-jetbrains.jar") else {
        eprintln!("jazyk: JetBrains LSP needs the built adapter and LSP4IJ; see bootstrap/editors/jetbrains/README.md");
        return Ok(());
    };
    let info = launch
        .as_ref()
        .and_then(|(program, args)| product_info(program, args));
    let Some((info_path, info)) = info else {
        eprintln!("jazyk: cannot identify this IDE's plugin directory; install LSP4IJ and the Jazyk adapter ZIP through Settings > Plugins");
        return Ok(());
    };
    let profile = info["dataDirectoryName"]
        .as_str()
        .ok_or("IDE product-info.json lacks dataDirectoryName")?;
    if profile.contains(['/', '\\']) || profile == ".." {
        return Err("invalid IDE data directory".into());
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or("cannot locate home directory")?;
    let plugins = jetbrains_plugins(Path::new(&home), profile);
    let installed = plugins.join("jazyk/lib/jazyk-jetbrains.jar");
    let mut needs_restart =
        write_atomic(&installed, &std::fs::read(jar).map_err(|e| e.to_string())?)?;
    let has_client = std::fs::read_dir(&plugins)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("lsp4ij")
        });
    if !has_client {
        // Installation uses the IDE's own marketplace compatibility checks.
        let current_os = if cfg!(target_os = "macos") {
            "macOS"
        } else if cfg!(target_os = "windows") {
            "Windows"
        } else {
            "Linux"
        };
        let launcher = info["launch"]
            .as_array()
            .and_then(|items| items.iter().find(|v| v["os"] == current_os))
            .and_then(|v| v["launcherPath"].as_str())
            .map(|s| info_path.parent().unwrap().join(s));
        if let Some(launcher) = launcher {
            println!("jazyk: installing the free LSP4IJ client for {profile}");
            let (properties, env_name) = installer_properties(root, &plugins, &info)?;
            match std::process::Command::new(launcher)
                .env(env_name, &properties)
                .args(["installPlugins", "com.redhat.devtools.lsp4ij"])
                .status()
            {
                Ok(status) if status.success() => {
                    needs_restart = true;
                    let _ = std::fs::remove_dir_all(properties.parent().unwrap());
                }
                result => eprintln!("jazyk: LSP4IJ installation did not complete ({result:?}); install it through Settings > Plugins. Installer logs: {}", properties.parent().unwrap().display()),
            }
        } else {
            eprintln!("jazyk: install LSP4IJ through Settings > Plugins");
        }
    }
    if needs_restart {
        println!(
            "jazyk: installed JetBrains LSP adapter; restart {profile} if it is already running"
        );
    }
    Ok(())
}

// A separate config/system pair avoids the open IDE's single-instance lock. The
// native IDE installer still selects a compatible marketplace release itself.
fn installer_properties(
    root: &Path,
    plugins: &Path,
    info: &Value,
) -> Result<(PathBuf, String), String> {
    let base = info["envVarBaseName"]
        .as_str()
        .ok_or("IDE product-info.json lacks envVarBaseName")?;
    if !base
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    {
        return Err("invalid IDE environment prefix".into());
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let dir = root
        .join(".jazyk/ide-installer")
        .join(format!("{}-{nonce}", std::process::id()));
    let mut properties = String::new();
    for (key, path) in [
        ("idea.config.path", dir.join("config")),
        ("idea.system.path", dir.join("system")),
        ("idea.log.path", dir.join("log")),
        ("idea.plugins.path", plugins.to_path_buf()),
    ] {
        // Java Properties Unicode escapes preserve spaces, backslashes, delimiters
        // and non-ASCII paths without depending on the launcher's text encoding.
        let value: String = path
            .to_string_lossy()
            .encode_utf16()
            .map(|c| format!("\\u{c:04x}"))
            .collect();
        properties.push_str(&format!("{key}={value}\n"));
    }
    let path = dir.join("installer.properties");
    write_atomic(&path, properties.as_bytes())?;
    Ok((path, format!("{base}_PROPERTIES")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zed_settings_preserve_comments_servers_and_other_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{
  // My editor settings
  "languages": {"Markdown": {
    // Keep my server order
    "language_servers": ["marksman", "!jazyk", "..."],
    "format_on_save": "off",
  }},
  "lsp": {"jazyk": {"settings": {"custom": true}}},
  "context_servers": {"existing": {"command": "other"}},
}"#,
        )
        .unwrap();
        assert!(zed_settings(&path, "/a path/jazyk").unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("// My editor settings"));
        assert!(text.contains("// Keep my server order"));
        let json = read_jsonc(&path).unwrap();
        assert_eq!(
            json["languages"]["Markdown"]["language_servers"],
            json!(["jazyk", "marksman", "..."])
        );
        assert_eq!(json["languages"]["Markdown"]["format_on_save"], "off");
        assert_eq!(json["lsp"]["jazyk"]["settings"]["custom"], true);
        assert_eq!(json["lsp"]["jazyk"]["binary"]["path"], "/a path/jazyk");
        assert_eq!(json["context_servers"]["existing"]["command"], "other");
        assert!(!zed_settings(&path, "/a path/jazyk").unwrap());
        assert!(zed_settings(&path, "/new/jazyk").unwrap());
    }

    #[test]
    fn zed_settings_create_and_reject_invalid_settings_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".zed/settings.json");
        zed_settings(&path, "jazyk").unwrap();
        assert_eq!(
            read_jsonc(&path).unwrap()["languages"]["Markdown"]["language_servers"],
            json!(["jazyk", "..."])
        );
        for text in [
            "not json",
            "[]",
            r#"{"languages":{"Markdown":{"language_servers":false}}}"#,
            r#"{"lsp":{"jazyk":{"binary":false}}}"#,
        ] {
            std::fs::write(&path, text).unwrap();
            assert!(zed_settings(&path, "jazyk").is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
        std::fs::write(&path, "// My project settings\n").unwrap();
        zed_settings(&path, "jazyk").unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .starts_with("// My project settings\n"));
    }

    #[test]
    fn detects_exact_jetbrains_product_profile() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("My IDEA.app");
        let path = app.join("Contents/Resources/product-info.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"dataDirectoryName":"IntelliJIdea2026.2"}"#).unwrap();
        let (_, info) = product_info(
            Path::new("open"),
            &[
                "-na".into(),
                app.to_string_lossy().into_owned(),
                "--args".into(),
            ],
        )
        .unwrap();
        assert_eq!(info["dataDirectoryName"], "IntelliJIdea2026.2");
    }
}
