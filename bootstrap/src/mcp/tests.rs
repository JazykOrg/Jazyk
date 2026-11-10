use super::server::{instructions_for, instructions_in};
use super::*;

mod catalog;
mod chat;
mod lifecycle;
mod project;
mod transport;
mod workers;

// The project-level chat serving, the one `jazyk mcp chat` builds at the root.
fn workspace_chat(dir: &std::path::Path, flags: BridgeFlags) -> McpServer {
    let project = crate::document_sets::Workspace::load(dir)
        .unwrap()
        .files_project();
    let out = project.out.clone();
    McpServer::with_bridge(project, out, vec!["chat".to_string()], false, flags)
}

// A refused tool call's message, whether the serving answered it as an isError
// result or a JSON-RPC error; panics on a call that succeeded.
fn refusal(reply: Result<Value, (i64, String)>) -> String {
    match reply {
        Err((_, message)) => message,
        Ok(v) if v["isError"] == true => v["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        Ok(v) => panic!("the call was not refused: {v}"),
    }
}

fn chat_server(dir: &std::path::Path) -> McpServer {
    let project = crate::project::Project::load(dir);
    let out = project.out.clone();
    McpServer::with_bridge(
        project,
        out,
        vec!["chat".to_string()],
        false,
        BridgeFlags::default(),
    )
}

fn tool_names(s: &McpServer) -> Vec<String> {
    let r = s.handle("tools/list", &json!({})).unwrap();
    r["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap_or_default().to_string())
        .collect()
}

fn bare_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("jazyk-mcp-{}-{}", tag, crate::sys::process_id()));
    crate::vfs::remove_dir_all(&dir).ok();
    crate::vfs::create_dir_all(dir.join("docs")).unwrap();
    crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
    dir
}
