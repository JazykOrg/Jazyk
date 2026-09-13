// The in-page session runner. Natively a session is an ACP worker session: jazyk
// spawns an agent, injects a `jazyk mcp` serving scoped to the batch, and the agent
// talks to it over a transport. In a browser tab there is nothing to spawn and
// nothing to connect over, so the same three pieces sit in one wasm module and call
// each other directly: the generic agent loop, the MCP serving it calls tools on,
// and the model in the page behind `crate::llm`.
//
// The serving is the same `McpServer` the native session gets, in the same compile
// mode, so the gates, the staged changeset, and the commit are the ones every other
// front door uses. See docs/frontends/wasm.md#sessions.

use crate::agent_loop::{self, AgentEvent, GenericTool, LoopArgs, Stop};
use crate::llm::Llm;
use crate::mcp::McpServer;
use crate::project::Project;
use crate::runner::{BatchRun, SessionReport, SessionRunner};
use crate::session::{Trace, TraceEvent};
use serde_json::{json, Value};
use std::cell::Cell;
use std::path::{Path, PathBuf};

pub struct InPageRunner {
    project: Project,
    llm: Llm,
    out: PathBuf,
    build_token: Cell<Option<String>>,
}

trait TakeClone {
    fn take_clone(&self) -> Option<String>;
}

impl TakeClone for Cell<Option<String>> {
    // Read a Cell<Option<String>> without moving out of it.
    fn take_clone(&self) -> Option<String> {
        let v = self.take();
        self.set(v.clone());
        v
    }
}

impl InPageRunner {
    pub fn start(project: &Project, llm: &Llm, out: &Path) -> Result<InPageRunner, String> {
        Ok(InPageRunner {
            project: project.clone(),
            llm: llm.clone(),
            out: out.to_path_buf(),
            build_token: Cell::new(None),
        })
    }

    // The tools the serving offers, in the shape the loop hands the model.
    fn tools_of(server: &McpServer) -> Result<Vec<GenericTool>, String> {
        let list = server
            .handle("tools/list", &json!({}))
            .map_err(|(_, m)| m)?;
        Ok(list["tools"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|t| GenericTool {
                name: t["name"].as_str().unwrap_or_default().to_string(),
                description: t["description"].as_str().unwrap_or_default().to_string(),
                parameters: t["inputSchema"].clone(),
            })
            .collect())
    }
}

impl SessionRunner for InPageRunner {
    fn agent_name(&self) -> String {
        "in-page".to_string()
    }

    // There is one build and one tab; the token still travels so the control plane
    // records the same shape it does natively.
    fn set_build_token(&self, token: Option<String>) {
        self.build_token.set(token);
    }

    fn run_item(&self, batch: &BatchRun, trace: &Trace) -> SessionReport {
        let label = batch.id.clone();
        let failed = |msg: String, rounds: u32, tokens: u64| SessionReport {
            applied: 0,
            rounds,
            tokens,
            failed: Some(msg),
            resolved: Vec::new(),
        };

        // The same flags the native runner gives its injected serving: it belongs
        // to this one batch (so the long poll is not offered, and end of input with
        // an open batch runs the implicit finish), it claims only this batch, and it
        // runs under the build's token so the build lease does not refuse it. The
        // prompt is not pre-packaged: begin_goals answers with it, and the loop
        // opens on that.
        let server = McpServer::with_bridge(
            self.project.clone(),
            self.out.clone(),
            vec!["compile".to_string()],
            true,
            crate::mcp::BridgeFlags {
                ephemeral: true,
                only: Some(batch.id.clone()),
                build_token: self.build_token.take_clone(),
                ..Default::default()
            },
        );
        if let Err(e) = server.handle(
            "initialize",
            &json!({"protocolVersion": "2024-11-05", "clientInfo": {"name": "jazyk-wasm"}}),
        ) {
            return failed(format!("serving: {}", e.1), 0, 0);
        }
        let tools = match Self::tools_of(&server) {
            Ok(t) => t,
            Err(e) => return failed(format!("serving: {}", e), 0, 0),
        };

        // Claim the batch. The reply carries the assembled session prompt, so the
        // prompt the model reads is the one every other front door assembles.
        let claim = match server.handle(
            "tools/call",
            &json!({"name": "begin_goals", "arguments": {"goals": batch.goal_ids()}}),
        ) {
            Ok(v) => v,
            Err(e) => return failed(format!("begin_goals: {}", e.1), 0, 0),
        };
        let opening = claim["content"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| claim.to_string());

        let gen_before = crate::store::read_generation(&self.out);
        let mut history: Vec<Value> = vec![json!({"role": "user", "content": opening})];
        let mut tokens = 0u64;
        let mut rounds = 0u32;

        let mut dispatch = |name: &str, args: &Value| -> Result<String, String> {
            match server.handle("tools/call", &json!({"name": name, "arguments": args})) {
                Ok(v) => {
                    let text = v["content"][0]["text"]
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| v.to_string());
                    if v["isError"].as_bool().unwrap_or(false) {
                        Err(text)
                    } else {
                        Ok(text)
                    }
                }
                Err((_, m)) => Err(m),
            }
        };
        let mut emit = |ev: AgentEvent| match ev {
            AgentEvent::Usage { tokens: t } => tokens += t,
            AgentEvent::ToolCallStart { name, args, .. } => {
                trace.event(TraceEvent::ToolCall {
                    label: label.clone(),
                    name,
                    summary: crate::session::condense(&args, 160),
                    full: crate::session::full_payload(&args),
                });
            }
            AgentEvent::Message(text) | AgentEvent::Thought(text) => {
                trace.event(TraceEvent::Note {
                    label: label.clone(),
                    text: crate::llm::truncate(&text, 400),
                    verbose: true,
                });
            }
            AgentEvent::ToolCallEnd { .. } => {}
        };

        let never_cancelled = || false;
        let stop = agent_loop::run_loop(LoopArgs {
            llm: &self.llm,
            history: &mut history,
            tools: &tools,
            dispatch: &mut dispatch,
            emit: &mut emit,
            cancelled: &never_cancelled,
            max_rounds: crate::limits::SESSION_ROUNDS,
            label: label.clone(),
        });
        rounds += 1;

        // The session's word is never the verdict: what landed is read from the
        // store, exactly as the native runner reads it.
        let gen_after = crate::store::read_generation(&self.out);
        let (applied, resolved) =
            crate::runner::journal_diff(&self.out, gen_before, gen_after, &batch.goal_ids());
        let committed = gen_after != gen_before;
        // A batch the model left open never commits: drop the changeset so the goals
        // return to open rather than sitting claimed.
        if !committed {
            let _ = server.handle(
                "tools/call",
                &json!({"name": "abandon_goals", "arguments": {"reason": "the session ended without committing"}}),
            );
        }

        let note = match stop {
            Stop::EndTurn => None,
            Stop::MaxRounds => Some(format!("the session ran out of rounds after {}", rounds)),
            Stop::Cancelled => Some("the session was cancelled".to_string()),
            Stop::Error(e) => Some(e),
        };
        SessionReport {
            applied,
            rounds,
            tokens,
            failed: if committed { None } else { note.or(Some("nothing committed".to_string())) },
            resolved,
        }
    }

    fn ask(&self, system: &str, user: &str, label: &str, step: &str) -> Result<String, String> {
        self.ask_traced(system, user, label, step, None)
    }

    fn ask_traced(
        &self,
        system: &str,
        user: &str,
        _label: &str,
        _step: &str,
        _trace: Option<&Trace>,
    ) -> Result<String, String> {
        self.llm.chat(system, user, _label, _step)
    }
}
