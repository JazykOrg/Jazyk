// The graph MCP server: the tool registry served over stdio as line-delimited JSON-RPC.
// Read tools by default; --write adds the mutation tools, each call committing as its own
// changeset. Mirrors docs2/frontends/mcp.md.
use crate::model::WorkItem;
use crate::store::Store;
use crate::tools::{catalog, toolset, ToolSession, WorkScope};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;

pub struct McpServer {
    out: PathBuf,
    write: bool,
    mutation_limit: usize,
    context_budget: usize,
}

impl McpServer {
    pub fn new(out: PathBuf, write: bool, limits: &crate::project::Limits) -> McpServer {
        McpServer {
            out,
            write,
            mutation_limit: limits.turn_mutations,
            context_budget: limits.context_budget,
        }
    }

    pub fn run(&self) {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
            let method = req["method"].as_str().unwrap_or_default().to_string();
            let id = req["id"].clone();
            if id.is_null() {
                continue; // notification, no response
            }
            let result = self.handle(&method, &req["params"]);
            let resp = match result {
                Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
                Err((code, msg)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}}),
            };
            let mut out = stdout.lock();
            writeln!(out, "{}", resp).ok();
            out.flush().ok();
        }
    }

    fn enabled_tools(&self) -> Vec<&'static str> {
        if self.write {
            toolset("mcp-write")
        } else {
            toolset("mcp-read")
        }
    }

    fn handle(&self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "jazyk", "version": env!("CARGO_PKG_VERSION")}
            })),
            "ping" => Ok(json!({})),
            "tools/list" => {
                let enabled = self.enabled_tools();
                let tools: Vec<Value> = catalog()
                    .iter()
                    .filter(|t| enabled.contains(&t.name))
                    .map(|t| json!({"name": t.name, "description": t.description, "inputSchema": t.parameters}))
                    .collect();
                Ok(json!({"tools": tools}))
            }
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default().to_string();
                let args = params["arguments"].clone();
                let enabled = self.enabled_tools();
                if !enabled.contains(&name.as_str()) {
                    return Err((-32602, format!("unknown or disabled tool `{}`", name)));
                }
                let store = Store::load(&self.out);
                if store.docs.is_empty() && store.graph.entities.is_empty() {
                    return Ok(text_result(
                        json!({"error": {"rule": "no-build", "message": "no graph found; run `jazyk compile` first"}}),
                        true,
                    ));
                }
                let is_write = !crate::tools::READ_TOOLS.contains(&name.as_str());
                let scope = WorkScope {
                    task: if is_write { "mcp-write".into() } else { "mcp-read".into() },
                    doc: None,
                    target_sections: Vec::new(),
                };
                let mut session = ToolSession::new(store, scope, self.mutation_limit, self.context_budget);
                match session.dispatch(&name, &args) {
                    Ok(v) => {
                        if is_write && !session.staged.is_empty() {
                            // Each MCP write commits as its own changeset, same gates, same journal.
                            let mut s = Store::load(&self.out);
                            let wi = WorkItem {
                                task: "mcp".into(),
                                target: name.clone(),
                                dirty_sections: vec![],
                                stale_anchors: vec![],
                            };
                            let report = s.apply(session.staged, &wi, 1, 0);
                            let mut v = v;
                            v["committed"] = json!(report.applied);
                            if !report.skipped.is_empty() {
                                v["skipped"] = json!(report.skipped);
                            }
                            return Ok(text_result(v, false));
                        }
                        Ok(text_result(v, false))
                    }
                    Err(e) => Ok(text_result(e.to_value(), true)),
                }
            }
            _ => Err((-32601, format!("method not found: {}", method))),
        }
    }
}

fn text_result(v: Value, is_error: bool) -> Value {
    json!({
        "content": [{"type": "text", "text": v.to_string()}],
        "isError": is_error
    })
}
