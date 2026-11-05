//! Native MCP subprocess I/O through the maintained Rust SDK.
pub use crate::harness::GenericTool;

use std::{future::Future, sync::mpsc, time::Duration};

use rmcp::{
    model::{CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation},
    service::RunningService,
    RoleClient, ServiceExt,
};
use serde_json::Value;

pub struct McpServerConn {
    pub name: String,
    service: Option<RunningService<RoleClient, ClientConfig>>,
    child: Option<tokio::process::Child>,
    pub tools: Vec<GenericTool>,
    pub instructions: Option<String>,
}

// Only owned transport operations run here. The caller's MCP dispatch and
// workspace scope never move. A channel also lets synchronous ACP lifecycle
// handlers call this while the ACP connection's futures executor is active.
fn io<T: Send + 'static>(
    future: impl Future<Output = Result<T, String>> + Send + 'static,
    timeout: Duration,
) -> Result<T, String> {
    let (tx, rx) = mpsc::sync_channel(1);
    let task = crate::llm::native_io_runtime().spawn(async move {
        let _ = tx.send(future.await);
    });
    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(error) => {
            task.abort();
            Err(format!("MCP I/O did not complete: {error}"))
        }
    }
}

impl McpServerConn {
    pub fn spawn(
        name: &str,
        command: &str,
        args: &[String],
        env: &[(String, String)],
        cwd: &std::path::Path,
    ) -> Result<Self, String> {
        let name = name.to_string();
        let operation_name = name.clone();
        let command = command.to_string();
        let args = args.to_vec();
        let env = env.to_vec();
        let cwd = cwd.to_path_buf();
        let timeout = crate::acp::config::idle_timeout();
        io(async move {
            let start = async {
                let mut process = tokio::process::Command::new(command);
                process.args(args).envs(env).current_dir(cwd).kill_on_drop(true)
                    .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::inherit());
                let mut child = process.spawn().map_err(|e| e.to_string())?;
                let stdout = child.stdout.take().ok_or("MCP server has no stdout")?;
                let stdin = child.stdin.take().ok_or("MCP server has no stdin")?;
                let client = ClientConfig::new(ClientCapabilities::default(), Implementation::new("jazyk-agent", env!("CARGO_PKG_VERSION")));
                let service = client.serve((stdout, stdin)).await.map_err(|e| e.to_string())?;
                let instructions = service.peer_info().and_then(|info| info.instructions.clone()).filter(|text| !text.trim().is_empty());
                let tools = service.list_all_tools().await.map_err(|e| e.to_string())?.into_iter()
                    .map(|tool| GenericTool { name: tool.name.into_owned(), description: tool.description.map(|d| d.into_owned()).unwrap_or_default(), parameters: Value::Object((*tool.input_schema).clone()) }).collect();
                Ok(Self { name: operation_name.clone(), service: Some(service), child: Some(child), tools, instructions })
            };
            tokio::time::timeout(timeout, start).await
                .map_err(|_| format!("MCP {operation_name}: initialization exceeded {}s (JAZYK_ACP_IDLE_TIMEOUT)", timeout.as_secs()))?
        }, timeout + Duration::from_secs(1)).map_err(|e| format!("MCP {name}: {e}"))
    }

    /// Connect a streamable HTTP server. Every request carries the hop header, so a
    /// jazyk-hosted endpoint can refuse a loop (docs/frontends/agent-context.md#hosted-context).
    pub fn connect_http(name: &str, url: &str, headers: &[(String, String)]) -> Result<Self, String> {
        use rmcp::transport::streamable_http_client::{StreamableHttpClientTransport, StreamableHttpClientTransportConfig};
        use reqwest::header::{HeaderName, HeaderValue};
        let mut map = std::collections::HashMap::new();
        for (k, v) in headers.iter().cloned().chain([(crate::agent_context::HOP_HEADER.to_string(), "1".to_string())]) {
            let key = HeaderName::from_bytes(k.as_bytes()).map_err(|e| format!("MCP {name}: header `{k}`: {e}"))?;
            let value = HeaderValue::from_str(&v).map_err(|e| format!("MCP {name}: header `{k}`: {e}"))?;
            map.insert(key, value);
        }
        let name = name.to_string();
        let operation_name = name.clone();
        let config = StreamableHttpClientTransportConfig::with_uri(url.to_string()).custom_headers(map);
        let timeout = crate::acp::config::idle_timeout();
        io(async move {
            let start = async {
                let transport = StreamableHttpClientTransport::from_config(config);
                let client = ClientConfig::new(ClientCapabilities::default(), Implementation::new("jazyk-agent", env!("CARGO_PKG_VERSION")));
                let service = client.serve(transport).await.map_err(|e| e.to_string())?;
                let instructions = service.peer_info().and_then(|info| info.instructions.clone()).filter(|text| !text.trim().is_empty());
                let tools = service.list_all_tools().await.map_err(|e| e.to_string())?.into_iter()
                    .map(|tool| GenericTool { name: tool.name.into_owned(), description: tool.description.map(|d| d.into_owned()).unwrap_or_default(), parameters: Value::Object((*tool.input_schema).clone()) }).collect();
                Ok(Self { name: operation_name.clone(), service: Some(service), child: None, tools, instructions })
            };
            tokio::time::timeout(timeout, start).await
                .map_err(|_| format!("MCP {operation_name}: initialization exceeded {}s (JAZYK_ACP_IDLE_TIMEOUT)", timeout.as_secs()))?
        }, timeout + Duration::from_secs(1)).map_err(|e| format!("MCP {name}: {e}"))
    }

    /// Fetch the current catalog after a batch claim or completion.
    pub fn refresh_tools(&mut self) -> Result<(), String> {
        let peer = self.service.as_ref().ok_or("MCP server is closed")?.peer().clone();
        let timeout = crate::acp::config::idle_timeout();
        let name = self.name.clone();
        self.tools = io(async move {
            let tools = tokio::time::timeout(timeout, peer.list_all_tools()).await
                .map_err(|_| format!("MCP {name}: tool listing exceeded {}s (JAZYK_ACP_IDLE_TIMEOUT)", timeout.as_secs()))?
                .map_err(|e| format!("MCP {name}: {e}"))?;
            Ok(tools.into_iter().map(|tool| GenericTool {
                name: tool.name.into_owned(),
                description: tool.description.map(|d| d.into_owned()).unwrap_or_default(),
                parameters: Value::Object((*tool.input_schema).clone()),
            }).collect())
        }, timeout + Duration::from_secs(1))?;
        Ok(())
    }

    /// Preserve the complete MCP result, including structured content and metadata.
    /// The GDK dynamic provider accepts this standard result envelope directly.
    pub fn call(&mut self, tool: &str, args: &Value) -> Result<String, String> {
        let peer = self
            .service
            .as_ref()
            .ok_or("MCP server is closed")?
            .peer()
            .clone();
        let params = CallToolRequestParams::new(tool.to_string()).with_arguments(
            args.as_object()
                .cloned()
                .ok_or("MCP tool arguments must be an object")?,
        );
        let timeout = crate::acp::config::idle_timeout();
        let name = self.name.clone();
        io(
            async move {
                let result = tokio::time::timeout(timeout, peer.call_tool(params))
                    .await
                    .map_err(|_| {
                        format!(
                            "MCP {name}: tool call exceeded {}s (JAZYK_ACP_IDLE_TIMEOUT)",
                            timeout.as_secs()
                        )
                    })?
                    .map_err(|e| format!("MCP {name}: {e}"))?;
                let failed = result.is_error.unwrap_or(false);
                let serialized = serde_json::to_string(&result).map_err(|e| e.to_string())?;
                if failed {
                    Err(serialized)
                } else {
                    Ok(serialized)
                }
            },
            timeout + Duration::from_secs(1),
        )
    }
}

impl Drop for McpServerConn {
    fn drop(&mut self) {
        if let Some(mut service) = self.service.take() {
            let child = self.child.take();
            // SDK close sends EOF, waits for the ephemeral serving to commit,
            // and reaps or kills a server that does not exit.
            let _ = io(
                async move {
                    service.close().await.map_err(|e| e.to_string())?;
                    if let Some(mut child) = child {
                        if tokio::time::timeout(Duration::from_secs(3), child.wait())
                            .await
                            .is_err()
                        {
                            child.kill().await.map_err(|e| e.to_string())?;
                        }
                    }
                    Ok(())
                },
                Duration::from_secs(5),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_client_keeps_instructions_tools_and_structured_results() {
        let script = r#"import json,sys
claimed=False
for line in sys.stdin:
 m=json.loads(line)
 if 'id' not in m: continue
 if m['method']=='initialize':
  r={'protocolVersion':m['params']['protocolVersion'],'capabilities':{'tools':{}},'serverInfo':{'name':'fixture','version':'1'},'instructions':'Read task_context before run_task.'}
 elif m['method']=='tools/list':
  r={'tools':[{'name':'scoped','inputSchema':{'type':'object'}}]} if claimed else ({'tools':[{'name':'fixture','description':'Read fixture','inputSchema':{'type':'object'}}],'nextCursor':'next'} if not m.get('params',{}).get('cursor') else {'tools':[{'name':'fixture_second','inputSchema':{'type':'object'}}]})
 elif m['method']=='tools/call':
  claimed=True
  r={'content':[{'type':'text','text':'visible text'}],'structuredContent':{'value':7},'_meta':{'provenance':'fixture'},'isError':False}
 else:r={}
 print(json.dumps({'jsonrpc':'2.0','id':m['id'],'result':r}),flush=True)
"#;
        let mut connection = McpServerConn::spawn(
            "fixture",
            "python3",
            &["-c".into(), script.into()],
            &[],
            &std::env::temp_dir(),
        )
        .unwrap();
        assert_eq!(
            connection.instructions.as_deref(),
            Some("Read task_context before run_task.")
        );
        assert_eq!(connection.tools.len(), 2);
        let result: Value =
            serde_json::from_str(&connection.call("fixture", &serde_json::json!({})).unwrap())
                .unwrap();
        assert_eq!(result["content"][0]["text"], "visible text");
        assert_eq!(result["structuredContent"]["value"], 7);
        assert_eq!(result["_meta"]["provenance"], "fixture");
        connection.refresh_tools().unwrap();
        assert_eq!(connection.tools.len(), 1);
        assert_eq!(connection.tools[0].name, "scoped");
    }

    #[test]
    fn sdk_close_delivers_eof_before_reaping_the_ephemeral_server() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("finished");
        let script = r#"import json,sys
for line in sys.stdin:
 m=json.loads(line)
 if 'id' not in m: continue
 r={'protocolVersion':m['params']['protocolVersion'],'capabilities':{'tools':{}},'serverInfo':{'name':'fixture','version':'1'}} if m['method']=='initialize' else {'tools':[]}
 print(json.dumps({'jsonrpc':'2.0','id':m['id'],'result':r}),flush=True)
open(sys.argv[1],'w').write('finished on EOF')
"#;
        let connection = McpServerConn::spawn(
            "fixture",
            "python3",
            &[
                "-c".into(),
                script.into(),
                marker.to_string_lossy().into_owned(),
            ],
            &[],
            directory.path(),
        )
        .unwrap();
        drop(connection);
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "finished on EOF");
    }
}
