pub use crate::agent_loop::GenericTool;
// Minimal sync MCP client over stdio, for the embedded agent: spawn each server a
// session names, list its tools, dispatch calls. Line-delimited JSON-RPC, the same
// framing the jazyk MCP server speaks. The embedded agent knows nothing about jazyk;
// this client treats every server identically.
// Mirrors docs/frontends/acp.md#the-embedded-agent.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

pub struct McpServerConn {
    pub name: String,
    child: Child,
    // Option so Drop can close it first: EOF is the server's shutdown signal, and a
    // jazyk `--ephemeral` serving runs its implicit finish on it.
    stdin: Option<ChildStdin>,
    // The server's lines, read on a thread so a request waits with a deadline; the
    // channel closes when the server does. Mirrors docs/frontends/acp.md#the-embedded-agent.
    lines: std::sync::mpsc::Receiver<String>,
    next_id: u64,
    pub tools: Vec<GenericTool>,
    pub instructions: Option<String>,
}

impl McpServerConn {
    pub fn spawn(
        name: &str,
        command: &str,
        args: &[String],
        env: &[(String, String)],
        cwd: &std::path::Path,
    ) -> Result<McpServerConn, String> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("cannot spawn MCP server `{}`: {}", command, e))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let mut stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let (tx, lines) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match stdout.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if tx.send(String::from_utf8_lossy(&buf).to_string()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let mut conn = McpServerConn {
            name: name.to_string(),
            child,
            stdin: Some(stdin),
            lines,
            next_id: 0,
            tools: Vec::new(),
            instructions: None,
        };
        let initialized = conn.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "jazyk-agent", "version": env!("CARGO_PKG_VERSION")}
            }),
        )?;
        conn.instructions = initialized["instructions"]
            .as_str()
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned);
        conn.notify("notifications/initialized", json!({}))?;
        let listed = conn.request("tools/list", json!({}))?;
        conn.tools = listed["tools"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|t| GenericTool {
                        name: t["name"].as_str().unwrap_or_default().to_string(),
                        description: t["description"].as_str().unwrap_or_default().to_string(),
                        parameters: if t["inputSchema"].is_null() {
                            json!({"type": "object"})
                        } else {
                            t["inputSchema"].clone()
                        },
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(conn)
    }

    // One tool call. The MCP result's content blocks concatenate into one string; an
    // isError result comes back as Err so the loop reports a failed call.
    pub fn call(&mut self, tool: &str, args: &Value) -> Result<String, String> {
        let result = self.request("tools/call", json!({"name": tool, "arguments": args}))?;
        let text = result["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        if result["isError"].as_bool().unwrap_or(false) {
            Err(text)
        } else {
            Ok(text)
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let stdin = self.stdin.as_mut().ok_or("mcp stdin closed")?;
        writeln!(stdin, "{}", line).map_err(|e| format!("mcp {} write: {}", self.name, e))?;
        stdin
            .flush()
            .map_err(|e| format!("mcp {} flush: {}", self.name, e))?;
        // Skip notifications and unrelated traffic until our response arrives, each
        // wait bounded: a wedged server fails the call instead of pinning the turn.
        let wait = crate::acp::config::idle_timeout();
        loop {
            let buf = match self.lines.recv_timeout(wait) {
                Ok(line) => line,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    return Err(format!(
                        "mcp {}: no answer to `{}` within {}s (JAZYK_ACP_IDLE_TIMEOUT)",
                        self.name,
                        method,
                        wait.as_secs()
                    ));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(format!(
                        "mcp {}: server closed the stream during `{}`",
                        self.name, method
                    ));
                }
            };
            let v: Value = match serde_json::from_str(buf.trim()) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v["id"].as_u64() != Some(id) {
                continue;
            }
            if !v["error"].is_null() {
                return Err(format!(
                    "mcp {} `{}`: {}",
                    self.name,
                    method,
                    v["error"]["message"].as_str().unwrap_or("error")
                ));
            }
            return Ok(v["result"].clone());
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let line = json!({"jsonrpc": "2.0", "method": method, "params": params});
        let stdin = self.stdin.as_mut().ok_or("mcp stdin closed")?;
        writeln!(stdin, "{}", line).map_err(|e| format!("mcp {} write: {}", self.name, e))?;
        stdin
            .flush()
            .map_err(|e| format!("mcp {} flush: {}", self.name, e))
    }
}

impl Drop for McpServerConn {
    fn drop(&mut self) {
        // Closing stdin is the shutdown signal: a jazyk `--ephemeral` serving runs its
        // implicit finish on EOF. Give it a moment to exit, then reap; kill only a
        // server that ignores the signal.
        drop(self.stdin.take());
        for _ in 0..50 {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
                Err(_) => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialization_keeps_server_instructions_beside_tools() {
        let script = r#"import json,sys
for line in sys.stdin:
 m=json.loads(line)
 if 'id' not in m: continue
 r={'instructions':'Read task_context before run_task.'} if m['method']=='initialize' else {'tools':[]}
 print(json.dumps({'jsonrpc':'2.0','id':m['id'],'result':r}),flush=True)
"#;
        let conn = McpServerConn::spawn(
            "fixture", "python3", &["-c".into(), script.into()], &[],
            &std::env::temp_dir(),
        ).unwrap();
        assert_eq!(conn.instructions.as_deref(), Some("Read task_context before run_task."));
        assert!(conn.tools.is_empty());
    }
}
