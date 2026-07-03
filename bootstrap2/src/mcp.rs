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
    project: crate::project::Project,
    out: PathBuf,
    write: bool,
    mutation_limit: usize,
    context_budget: usize,
}

impl McpServer {
    pub fn new(project: crate::project::Project, out: PathBuf, write: bool) -> McpServer {
        McpServer {
            mutation_limit: project.limits.turn_mutations,
            context_budget: project.limits.context_budget,
            project,
            out,
            write,
        }
    }

    // The server's own long poll: returns when the graph's generation counter moves or a
    // documentation file changes on disk, or at the timeout. Mirrors
    // docs2/frontends/mcp.md#external-generation-workers.
    fn await_changes(&self, params: &Value) -> Value {
        let timeout = params["arguments"]["timeout_seconds"].as_u64().unwrap_or(300).clamp(1, 3600);
        let fingerprint = |path: &std::path::Path| -> String {
            std::fs::metadata(path)
                .map(|m| format!("{}:{:?}", m.len(), m.modified().ok()))
                .unwrap_or_default()
        };
        let snapshot: std::collections::BTreeMap<std::path::PathBuf, String> = self
            .project
            .doc_files()
            .into_iter()
            .map(|f| (f.clone(), fingerprint(&f)))
            .collect();
        let start_gen = Store::load(&self.out).status.generation;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
        let mut changed_docs: Vec<String> = Vec::new();
        let mut changed = false;
        while std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let store = Store::load(&self.out);
            if store.status.generation != start_gen {
                changed = true;
            }
            for f in self.project.doc_files() {
                if snapshot.get(&f).map(|s| s.as_str()) != Some(fingerprint(&f).as_str()) {
                    let rel = f
                        .strip_prefix(&self.project.root)
                        .map(|r| r.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_else(|_| f.to_string_lossy().to_string());
                    if !changed_docs.contains(&rel) {
                        changed_docs.push(rel);
                    }
                    changed = true;
                }
            }
            if changed {
                break;
            }
        }
        let store = Store::load(&self.out);
        // Stale: a documentation file's content no longer matches what the graph reconciled.
        let graph_stale = self.project.doc_files().iter().any(|f| {
            let rel = f
                .strip_prefix(&self.project.root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            match (std::fs::read_to_string(f), store.docs.get(&rel)) {
                (Ok(text), Some(rec)) => crate::model::hash_hex(&text) != rec.content_hash,
                (Ok(_), None) => true,
                _ => false,
            }
        });
        json!({
            "changed": changed,
            "changedDocs": changed_docs,
            "graphStale": graph_stale,
            "generation": store.status.generation,
            "pending": crate::gen::pending(&store, "rust"),
        })
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
                let mut tools: Vec<Value> = catalog()
                    .iter()
                    .filter(|t| enabled.contains(&t.name))
                    .map(|t| json!({"name": t.name, "description": t.description, "inputSchema": t.parameters}))
                    .collect();
                tools.push(json!({
                    "name": "await_changes",
                    "description": "Long poll: returns when the graph's generation counter moves or a documentation file changes on disk, or at the timeout (default 300s). Carries the changed documents, graph staleness, and pending generation work.",
                    "inputSchema": {"type": "object", "properties": {"timeout_seconds": {"type": "integer"}}, "additionalProperties": false}
                }));
                Ok(json!({"tools": tools}))
            }
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default().to_string();
                if name == "await_changes" {
                    return Ok(text_result(self.await_changes(params), false));
                }
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
