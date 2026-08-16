// Registering jazyk as an ACP agent in the editors that host one. Every client
// spawns the agent as a child process over stdio, so a registration is always the
// same three facts (a name, a command, its arguments) written wherever that client
// keeps them. Mirrors docs/frontends/acp.md#registration.
use serde_json::{json, Value};
use std::path::PathBuf;

// What a client's config asks for, and where it lives.
pub enum Registry {
    // A JSON or JSONC settings file holding a map of agent name to spawn entry.
    // Merged in place: comments and formatting survive, other agents are untouched.
    Map { path: PathBuf, key: &'static str, entry: Value },
    // A config jazyk must not write: editor-managed state, or a language it would
    // have to parse to edit safely. The user pastes the snippet.
    Snippet { where_: String, text: String },
}

pub struct Ide {
    pub id: &'static str,
    pub label: &'static str,
    pub registry: Registry,
}

// Every client jazyk knows how to register with, in the order `jazyk init` offers
// them. Mirrors docs/frontends/acp.md#registration.
pub const IDES: [&str; 8] =
    ["zed", "jetbrains", "vscode", "neovim", "emacs", "obsidian", "acpx", "marimo"];

pub fn ide(id: &str, cmd: &str) -> Option<Ide> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let stdio = |extra: Value| {
        let mut e = json!({"command": cmd, "args": ["acp"]});
        if let (Some(o), Some(x)) = (e.as_object_mut(), extra.as_object()) {
            for (k, v) in x {
                o.insert(k.clone(), v.clone());
            }
        }
        e
    };
    Some(match id {
        // Zed names the kind of agent server explicitly; a custom binary is "custom".
        "zed" => Ide {
            id: "zed",
            label: "Zed",
            registry: Registry::Map {
                path: home.join(".config/zed/settings.json"),
                key: "agent_servers",
                entry: stdio(json!({"type": "custom"})),
            },
        },
        "jetbrains" => Ide {
            id: "jetbrains",
            label: "JetBrains IDEs",
            registry: Registry::Map {
                path: home.join(".jetbrains/acp.json"),
                key: "agent_servers",
                entry: stdio(json!({})),
            },
        },
        // VS Code has no ACP host of its own; the ACP Client extension provides one
        // and reads normal user settings.
        "vscode" => Ide {
            id: "vscode",
            label: "VS Code (ACP Client extension)",
            registry: Registry::Map {
                path: vscode_settings(&home),
                key: "acp.agents",
                entry: stdio(json!({})),
            },
        },
        "neovim" => Ide {
            id: "neovim",
            label: "Neovim (CodeCompanion.nvim)",
            registry: Registry::Snippet {
                where_: "your CodeCompanion setup (init.lua)".into(),
                text: format!(
                    "require(\"codecompanion\").setup({{\n  \
                     adapters = {{\n    acp = {{\n      jazyk = function()\n        \
                     return require(\"codecompanion.adapters\").extend(\"gemini_cli\", {{\n          \
                     name = \"jazyk\",\n          formatted_name = \"Jazyk\",\n          \
                     commands = {{ default = {{ \"{}\", \"acp\" }} }},\n        \
                     }})\n      end,\n    }},\n  }},\n}})",
                    cmd
                ),
            },
        },
        "emacs" => Ide {
            id: "emacs",
            label: "Emacs (agent-shell)",
            registry: Registry::Snippet {
                where_: "your Emacs configuration".into(),
                text: format!(
                    "(add-to-list 'agent-shell-agent-configs\n  \
                     (agent-shell-make-agent-config\n   \
                     :identifier 'jazyk\n   :mode-line-name \"Jazyk\"\n   \
                     :buffer-name \"Jazyk\"\n   :shell-prompt \"Jazyk> \"\n   \
                     :shell-prompt-regexp \"Jazyk> \"\n   \
                     :client-maker (lambda (buffer)\n                   \
                     (agent-shell--make-acp-client\n                    \
                     :command \"{}\"\n                    :command-params '(\"acp\")\n                    \
                     :context-buffer buffer))))",
                    cmd
                ),
            },
        },
        // The Obsidian plugin keeps its agents in vault-local plugin state it owns;
        // jazyk states the fields instead of writing another program's data file.
        "obsidian" => Ide {
            id: "obsidian",
            label: "Obsidian (Agent Client plugin)",
            registry: Registry::Snippet {
                where_: "Settings, Agent Client, Custom Agents, Add custom agent".into(),
                text: format!(
                    "Agent ID:   jazyk\nDisplay name: Jazyk\nPath:       {}\nArguments:  acp",
                    cmd
                ),
            },
        },
        "acpx" => Ide {
            id: "acpx",
            label: "acpx (scripting client)",
            registry: Registry::Snippet {
                where_: "a shell".into(),
                text: format!("acpx --agent '{} acp' \"your prompt\"", cmd),
            },
        },
        // marimo speaks ACP over a WebSocket, so the agent runs behind a bridge.
        "marimo" => Ide {
            id: "marimo",
            label: "marimo notebooks",
            registry: Registry::Snippet {
                where_: "a terminal, then enable the agent in marimo's settings".into(),
                text: format!("npx stdio-to-ws \"{} acp\" --port 3017", cmd),
            },
        },
        _ => return None,
    })
}

// VS Code's user settings live in a per-platform application directory.
fn vscode_settings(home: &std::path::Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Code/User/settings.json")
    } else if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("Code/User/settings.json")
    } else {
        home.join(".config/Code/User/settings.json")
    }
}

// The command an editor should spawn. What the user typed is what belongs in the
// config: `jazyk`, not the release binary it currently resolves to, so a rebuild or
// a moved checkout does not strand the registration. The absolute path is the
// fallback for a binary that is not on PATH.
// Mirrors docs/frontends/acp.md#registration.
pub fn spawn_command() -> String {
    let exe = std::env::current_exe().ok();
    let canonical = exe.as_ref().and_then(|p| std::fs::canonicalize(p).ok());
    let argv0 = std::env::args().next().unwrap_or_default();
    let name = std::path::Path::new(&argv0)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("jazyk")
        .to_string();
    if let (Some(found), Some(canonical)) = (which(&name), canonical.as_ref()) {
        if std::fs::canonicalize(&found).ok().as_ref() == Some(canonical) {
            return name;
        }
    }
    exe.and_then(|p| p.to_str().map(|s| s.to_string())).unwrap_or(name)
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}

pub fn install(ide_id: &str) -> i32 {
    let cmd = spawn_command();
    let Some(ide) = ide(ide_id, &cmd) else {
        eprintln!("jazyk: unknown editor `{}`; one of {}", ide_id, IDES.join(", "));
        return 2;
    };
    match ide.registry {
        Registry::Snippet { where_, text } => {
            println!("jazyk: {} keeps its agents where jazyk should not write.", ide.label);
            println!("Add this in {}:\n\n{}\n", where_, text);
            0
        }
        Registry::Map { path, key, entry } => match merge_into(&path, key, "Jazyk", &entry) {
            Ok(Merge::Unchanged) => {
                println!("jazyk: {} already registers Jazyk ({})", ide.label, path.display());
                0
            }
            Ok(Merge::Written) => {
                println!("jazyk: registered Jazyk in {}", path.display());
                0
            }
            Err(e) => {
                eprintln!("jazyk: cannot register in {}: {}", path.display(), e);
                eprintln!("add this by hand under `{}`:\n  \"Jazyk\": {}", key, entry);
                1
            }
        },
    }
}

pub enum Merge {
    Written,
    Unchanged,
}

// Merge one agent entry into a settings file, in place. Editors keep these files by
// hand: they carry comments, trailing commas, and an author's formatting, so the
// merge is a text splice at the parsed node's range and never a re-serialization of
// the document. Mirrors docs/frontends/acp.md#registration.
pub fn merge_into(path: &std::path::Path, key: &str, name: &str, entry: &Value) -> Result<Merge, String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let updated = splice(&text, key, name, entry)?;
    let Some(updated) = updated else { return Ok(Merge::Unchanged) };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, updated).map_err(|e| e.to_string())?;
    Ok(Merge::Written)
}

// Returns the new document, or None when the entry is already exactly there.
fn splice(text: &str, key: &str, name: &str, entry: &Value) -> Result<Option<String>, String> {
    use jsonc_parser::ast::{ObjectProp, Value as JValue};
    use jsonc_parser::{parse_to_ast, CollectOptions, CommentCollectionStrategy, ParseOptions};

    if text.trim().is_empty() {
        return Ok(Some(format!(
            "{{\n  {}: {{\n    {}: {}\n  }}\n}}\n",
            json!(key),
            json!(name),
            indented(entry, 4)
        )));
    }
    let parse_options = ParseOptions {
        allow_comments: true,
        allow_trailing_commas: true,
        allow_loose_object_property_names: true,
        ..Default::default()
    };
    let ast = parse_to_ast(
        text,
        &CollectOptions { comments: CommentCollectionStrategy::Off, tokens: false },
        &parse_options,
    )
    .map_err(|e| e.to_string())?;
    let Some(JValue::Object(root)) = ast.value.as_ref() else {
        return Err("the file does not hold a JSON object".into());
    };
    let rendered = |indent: usize| -> String { indented(entry, indent) };

    // The map already exists: replace our entry, or insert beside its siblings.
    if let Some(prop) = root.get(key) {
        let JValue::Object(map) = &prop.value else {
            return Err(format!("`{}` is not an object", key));
        };
        let inner_indent = indent_of(text, map.properties.first().map(|p| p.range.start))
            .unwrap_or_else(|| indent_of(text, Some(prop.range.start)).unwrap_or(2) * 2);
        if let Some(existing) = map.get(name) {
            let ObjectProp { value, .. } = existing;
            let (start, end) = (range_of(value).start, range_of(value).end);
            if same_json(&text[start..end], entry) {
                return Ok(None);
            }
            let mut out = String::with_capacity(text.len());
            out.push_str(&text[..start]);
            out.push_str(&rendered(inner_indent));
            out.push_str(&text[end..]);
            return Ok(Some(out));
        }
        let at = map.range.start + 1;
        let mut out = String::with_capacity(text.len());
        out.push_str(&text[..at]);
        out.push('\n');
        out.push_str(&" ".repeat(inner_indent));
        out.push_str(&format!("\"{}\": {}", name, rendered(inner_indent)));
        if !map.properties.is_empty() {
            out.push(',');
        }
        out.push_str(&text[at..]);
        return Ok(Some(out));
    }

    // No map yet: add it at the top of the document, where a reader will find it.
    let outer_indent = indent_of(text, root.properties.first().map(|p| p.range.start)).unwrap_or(2);
    let at = root.range.start + 1;
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..at]);
    out.push('\n');
    out.push_str(&" ".repeat(outer_indent));
    out.push_str(&format!(
        "\"{}\": {{\n{}\"{}\": {}\n{}}}",
        key,
        " ".repeat(outer_indent * 2),
        name,
        indented(entry, outer_indent * 2),
        " ".repeat(outer_indent),
    ));
    if !root.properties.is_empty() {
        out.push(',');
    }
    out.push_str(&text[at..]);
    Ok(Some(out))
}

fn range_of(v: &jsonc_parser::ast::Value) -> jsonc_parser::common::Range {
    use jsonc_parser::ast::Value as JValue;
    match v {
        JValue::StringLit(n) => n.range,
        JValue::NumberLit(n) => n.range,
        JValue::BooleanLit(n) => n.range,
        JValue::Object(n) => n.range,
        JValue::Array(n) => n.range,
        JValue::NullKeyword(n) => n.range,
    }
}

// Whether the text already says what the entry says, formatting aside.
fn same_json(text: &str, entry: &Value) -> bool {
    jsonc_parser::parse_to_serde_value::<Value>(text, &Default::default())
        .map(|v| &v == entry)
        .unwrap_or(false)
}

// The indentation of the line a node starts on, in spaces.
fn indent_of(text: &str, at: Option<usize>) -> Option<usize> {
    let at = at?;
    let line_start = text[..at].rfind('\n').map(|i| i + 1)?;
    let spaces = text[line_start..at].chars().take_while(|c| *c == ' ').count();
    (spaces > 0).then_some(spaces)
}

// The entry as an editor's own config would write it: one field per line at the
// caller's indentation, values compact, so `"args": ["acp"]` stays on one line.
fn indented(v: &Value, indent: usize) -> String {
    let Some(fields) = v.as_object() else {
        return v.to_string();
    };
    let pad = " ".repeat(indent);
    let inner = " ".repeat(indent + 2);
    let body: Vec<String> =
        fields.iter().map(|(k, val)| format!("{}{}: {}", inner, json!(k), val)).collect();
    format!("{{\n{}\n{}}}", body.join(",\n"), pad)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> Value {
        json!({"type": "custom", "command": "jazyk", "args": ["acp"]})
    }

    // A hand-kept settings file keeps its comments, its other agents, and its
    // formatting; only the Jazyk entry appears.
    // Mirrors docs/frontends/acp.md#registration.
    #[test]
    fn merging_preserves_comments_and_siblings() {
        let text = "{\n  // my editor settings\n  \"theme\": \"dark\",\n  \"agent_servers\": {\n    \"Gemini\": { \"command\": \"gemini\" }\n  }\n}\n";
        let out = splice(text, "agent_servers", "Jazyk", &entry()).unwrap().unwrap();
        assert!(out.contains("// my editor settings"), "{}", out);
        assert!(out.contains("\"Gemini\""), "{}", out);
        assert!(out.contains("\"type\": \"custom\""), "{}", out);
        let parsed: Value =
            jsonc_parser::parse_to_serde_value(&out, &Default::default()).expect("valid jsonc");
        assert_eq!(parsed["agent_servers"]["Jazyk"], entry());
        assert_eq!(parsed["agent_servers"]["Gemini"]["command"], "gemini");
        assert_eq!(parsed["theme"], "dark");
    }

    // Re-running the install is not a rewrite, and a stale entry is corrected in
    // place rather than duplicated.
    #[test]
    fn merging_is_idempotent_and_repairs_a_stale_entry() {
        let text = "{\n  \"agent_servers\": {\n    \"Jazyk\": { \"command\": \"/old/path/jazyk\", \"args\": [\"acp\"] }\n  }\n}\n";
        let out = splice(text, "agent_servers", "Jazyk", &entry()).unwrap().unwrap();
        let parsed: Value = jsonc_parser::parse_to_serde_value(&out, &Default::default()).unwrap();
        assert_eq!(parsed["agent_servers"]["Jazyk"], entry());
        assert!(!out.contains("/old/path/jazyk"), "{}", out);
        assert!(splice(&out, "agent_servers", "Jazyk", &entry()).unwrap().is_none());
    }

    // Files that have no map yet, including one that does not exist at all.
    #[test]
    fn merging_creates_the_map_and_the_document() {
        let out = splice("{\n  \"theme\": \"dark\"\n}\n", "acp.agents", "Jazyk", &entry())
            .unwrap()
            .unwrap();
        let parsed: Value = jsonc_parser::parse_to_serde_value(&out, &Default::default()).unwrap();
        assert_eq!(parsed["acp.agents"]["Jazyk"], entry());
        assert_eq!(parsed["theme"], "dark");

        let fresh = splice("", "agent_servers", "Jazyk", &entry()).unwrap().unwrap();
        let parsed: Value = serde_json::from_str(&fresh).unwrap();
        assert_eq!(parsed["agent_servers"]["Jazyk"], entry());

        // A trailing comma is normal in a hand-kept JSONC file, not an error.
        let loose = splice("{\n  \"agent_servers\": {\n  },\n}\n", "agent_servers", "Jazyk", &entry())
            .unwrap()
            .unwrap();
        let parsed: Value = jsonc_parser::parse_to_serde_value(&loose, &Default::default()).unwrap();
        assert_eq!(parsed["agent_servers"]["Jazyk"], entry());
    }

    // The registered command is a name, not a path, whenever the name resolves to
    // this same binary: a rebuild must not strand the registration.
    #[test]
    fn every_ide_registers_the_same_command() {
        let cmd = spawn_command();
        assert!(!cmd.is_empty());
        for id in IDES {
            let ide = ide(id, &cmd).expect(id);
            match ide.registry {
                Registry::Map { entry, .. } => assert_eq!(entry["command"], cmd),
                Registry::Snippet { text, .. } => assert!(text.contains(&cmd), "{}: {}", id, text),
            }
        }
    }
}
