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

use crate::harness::{self, AgentEvent, GenericTool, LoopArgs, Stop};
use crate::llm::Llm;
use crate::mcp::McpServer;
use crate::project::Project;
use crate::runner::{BatchRun, LoopTrace, SessionReport, SessionRunner};
use crate::session::{Trace, TraceEvent};
use serde_json::{json, Value};
use std::cell::Cell;
use std::path::{Path, PathBuf};

fn ledger_failure(stop: &Stop, rounds: u32) -> Option<String> {
    match stop {
        Stop::Error(error) if !crate::session_recovery::is_interruption(error) => {
            Some(crate::runner::unlanded_reason(stop, rounds))
        }
        _ => None,
    }
}
pub struct InProcessRunner {
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

impl InProcessRunner {
    pub fn start(project: &Project, llm: &Llm, out: &Path) -> Result<InProcessRunner, String> {
        Ok(InProcessRunner {
            project: project.clone(),
            llm: llm.clone(),
            out: out.to_path_buf(),
            build_token: Cell::new(None),
        })
    }

    fn serving_for(&self, batch: &BatchRun) -> McpServer {
        McpServer::with_bridge(
            self.project.clone(), self.out.clone(), vec![batch.serving_mode().into()], true,
            crate::mcp::BridgeFlags {
                ephemeral: true,
                serve_files: matches!(batch.serving_mode(), "generate" | "verify"),
                only: Some(batch.id.clone()),
                build_token: self.build_token.take_clone(),
                ..Default::default()
            },
        )
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

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl SessionRunner for InProcessRunner {
    fn agent_name(&self) -> String {
        "in-page".to_string()
    }

    // There is one build and one tab; the token still travels so the control plane
    // records the same shape it does natively.
    fn set_build_token(&self, token: Option<String>) {
        self.build_token.set(token);
    }

    async fn run_item(&self, batch: &BatchRun, trace: &Trace) -> SessionReport {
        let label = batch.id.clone();
        let failed = |msg: String, rounds: u32, tokens: u64| SessionReport {
            applied: 0,
            rounds,
            tokens,
            cached: 0,
            failed: Some(msg.clone()),
            resolved: Vec::new(),
            stopped: crate::session_recovery::is_interruption(&msg).then_some(msg.clone()),
        };

        // The same flags the native runner gives its injected serving: it belongs
        // to this one batch (so the long poll is not offered, and end of input with
        // an open batch runs the implicit finish), it claims only this batch, and it
        // runs under the build's token so the build lease does not refuse it. The
        // prompt is not pre-packaged: begin_goals answers with it, and the loop
        // opens on that.
        let server = self.serving_for(batch);
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

        let recovery_key = crate::session_recovery::batch_key(&self.out, &batch.id);
        let mut recovery = match crate::session_recovery::McpRecovery::new(&server, recovery_key.clone()) {
            Ok(recovery) => recovery,
            Err(error) => return failed(error, 0, 0),
        };
        let opening = if recovery.resumed() {
            String::new()
        } else {
            let claim = match server.handle(
                "tools/call",
                &json!({"name": "begin_goals", "arguments": {"goals": batch.goal_ids()}}),
            ) {
                Ok(v) => v,
                Err(e) => return failed(format!("begin_goals: {}", e.1), 0, 0),
            };
            claim["content"][0]["text"].as_str().map(str::to_owned).unwrap_or_else(|| claim.to_string())
        };

        let opening = if batch.is_ledger() {
            format!(
                "{}\n\n{}",
                crate::runner::batch_prompt(&self.project, &self.out, batch),
                opening
            )
        } else {
            opening
        };

        let gen_before = crate::store::read_generation(&self.out);
        let mut history: Vec<Value> = vec![json!({"role": "user", "content": opening})];
        // The session's trail on the build trace, as a native worker session leaves
        // it (docs/frontends/wasm.md#sessions). The claim above precedes it: a batch
        // that cannot be claimed never opened a session.
        trace.event(crate::runner::session_start(batch));
        let mut lt = LoopTrace::new(trace, batch);
        let mut preview_revision = 0;
        if let Some(preview) = server.live_preview() {
            preview_revision = preview.revision;
            trace.event(TraceEvent::GraphPreview { preview });
        }

        let mut dispatch = |_: &str, _: &Value| Err("MCP requires its asynchronous host".into());
        let mut model = harness::provider::DefaultModelHost::new(&self.llm);
        let mut mcp_tools = crate::session_recovery::McpTools::plain(&server);
        let owner = crate::conversation::build_conversation();
        let mut emit = |ev: AgentEvent| {
            if let (Some(id), AgentEvent::Input(input)) = (&owner, &ev) {
                crate::conversation::build_input_delivered(id, input.clone());
            }
            if matches!(&ev, AgentEvent::ToolCallEnd { .. }) {
                if let Some(preview) = server.live_preview() {
                    if preview.revision > preview_revision {
                        preview_revision = preview.revision;
                        trace.event(TraceEvent::GraphPreview { preview });
                    }
                }
            }
            lt.on_event(ev);
        };
        let mut input_source = || owner.as_deref()
            .map(crate::conversation::build_inputs).unwrap_or_default();

        let never_cancelled = || trace.is_cancelled();
        let stop = harness::run_with_hosts(LoopArgs {
            llm: &self.llm,
            history: &mut history,
            tools: &tools,
            dispatch: &mut dispatch,
            emit: &mut emit,
            cancelled: &never_cancelled,
            max_rounds: crate::limits::agent_max_rounds(),
            label: label.clone(),
            conversation: false,
        inputs: owner.as_ref().map(|_| &mut input_source as &mut dyn FnMut() -> Vec<Value>),
        // This is one worker batch, not the whole job's admission lifetime.
        input_close: None,
        }, "", &mut model, &mut mcp_tools, &mut recovery).await;
        lt.finish();
        let (tokens, cached, rounds) = crate::session_recovery::saved_usage(&recovery_key)
            .unwrap_or((lt.tokens, lt.cached, lt.rounds));

        // The session's word is never the verdict: what landed is read from the
        // store, exactly as the native runner reads it.
        let gen_after = crate::store::read_generation(&self.out);
        let (applied, resolved) =
            crate::runner::journal_diff(&self.out, gen_before, gen_after, &batch.goal_ids());
        let committed = gen_after != gen_before;
        let interruption = match &stop {
            Stop::Suspended => Some("suspended".to_owned()),
            Stop::Error(error) if crate::session_recovery::is_interruption(error) => Some(error.clone()),
            _ => None,
        };
        if trace.is_cancelled() {
            // User Stop wins over a simultaneous model/checkpoint interruption.
            // Earlier commits do not grant permission to retain a new open batch.
            server.cancel_owned();
        } else if let Some(reason) = interruption {
            return SessionReport {
                applied, rounds, tokens, cached, failed: None, resolved,
                stopped: Some(reason),
            };
        }
        // A batch the model left open never commits: drop the changeset so the goals
        // return to open rather than sitting claimed.
        if !committed {
            let _ = server.handle(
                "tools/call",
                &json!({"name": "abandon_goals", "arguments": {"reason": "the session ended without committing"}}),
            );
        }

        // Discard any abandoned staged display and publish the actual session base.
        let store = crate::store::Store::load(&self.out);
        trace.event(TraceEvent::GraphPreview { preview: crate::live_preview::Preview {
            label: label.clone(), attempt: server.live_preview().map(|p|p.attempt).unwrap_or_default(),
            revision: preview_revision + 1, phase: "committed".into(),
            generation: store.status.generation, graph: Some(crate::live_preview::authored(&store.graph)), reason: None,
        }.bounded() });

        // Why the session stopped short of a plain end of turn, for a caller judging
        // its own ledger; a batch that did not land fails with the stop in words
        // (docs/compiler/sessions.md#trace-events).
        let note = match &stop {
            Stop::EndTurn => None,
            other => Some(crate::runner::unlanded_reason(other, rounds)),
        };
        if trace.is_cancelled() && !committed {
            trace.event(TraceEvent::SessionEnded { label: label.clone(), goals: batch.goal_ids(), reason: "the build was cancelled".into() });
        }
        if committed {
            // The batch landed: the store's word. A failure's terminal event is the
            // reconciler's, which knows the attempt number.
            trace.event(TraceEvent::SessionDone {
                label: label.clone(),
                goals: batch.goal_ids(),
                staged: applied,
                rounds,
                mode: "done".into(),
                summary: String::new(),
            });
        }
        SessionReport {
            applied,
            rounds,
            tokens,
            cached,
            failed: if trace.is_cancelled() {
                None
            } else if batch.is_ledger() {
                // The ledger worker checks its gates first, including partial
                // commits. Only a runtime error is a fatal session failure here.
                ledger_failure(&stop, rounds)
            } else if committed {
                None
            } else {
                Some(crate::runner::unlanded_reason(&stop, rounds))
            },
            resolved,
            stopped: note,
        }
    }

    async fn ask(&self, system: &str, user: &str, label: &str, step: &str) -> Result<String, String> {
        self.ask_traced(system, user, label, step, None).await
    }

    async fn ask_traced(
        &self,
        system: &str,
        user: &str,
        _label: &str,
        _step: &str,
        _trace: Option<&Trace>,
    ) -> Result<String, String> {
        self.llm.chat(system, user, _label, _step).await
    }
}

#[cfg(test)]
mod ledger_failure_tests {
    use super::*;

    #[test]
    fn verification_runner_can_read_implementing_files_without_file_or_matcher_writes() {
        let _fs = crate::vfs::Scope::enter(&Default::default());
        crate::vfs::create_dir_all("/project/.jazyk").unwrap();
        crate::vfs::create_dir_all("/project/src").unwrap();
        crate::vfs::write("/project/.jazyk/jazyk.toml", "version=1\n[[sets]]\nid='ds_main'\nname='Main'\ndocs=['docs/**']\nsource=['src/**']\n").unwrap();
        crate::vfs::write("/project/src/greeting.txt", "Hello").unwrap();
        let project = Project::load(Path::new("/project"));
        let mut store = crate::store::Store::load(&project.out);
        store.graph.entities.insert("ent:greeter".into(), crate::model::Entity { name:"Greeter".into(), ..Default::default() });
        store.save();
        let llm = Llm { api: "chat_completions".into(), reasoning_summary: None,  base_url:"http://127.0.0.1:1/v1".into(), model:"unused".into(), api_key:String::new(), temperature:None, trace:None };
        let runner = InProcessRunner::start(&project,&llm,&project.out).unwrap();
        let batch = BatchRun::single(crate::model::Goal { id:"g:verify:req:greet".into(),kind:"verify".into(),target:"req:greet".into(),..Default::default() });
        let server = runner.serving_for(&batch);
        let tools = InProcessRunner::tools_of(&server).unwrap();
        for name in ["read_text_file","list_files"] { assert!(tools.iter().any(|tool| tool.name == name)); }
        for name in ["write_text_file","update_document_set","run_command"] { assert!(!tools.iter().any(|tool| tool.name == name)); }
        let read = server.handle("tools/call",&json!({"name":"read_text_file","arguments":{"path":"src/greeting.txt"}})).unwrap();
        assert_ne!(read["isError"],true,"{read}");
        assert!(read["content"][0]["text"].as_str().unwrap().contains("Hello"));
        assert!(server.handle("tools/call",&json!({"name":"write_text_file","arguments":{"path":"src/greeting.txt","content":"Changed"}})).is_err());
        assert!(server.handle("tools/call",&json!({"name":"update_document_set","arguments":{}})).is_err());
        assert_eq!(crate::vfs::read_to_string("/project/src/greeting.txt").unwrap(),"Hello");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn generation_can_record_after_twenty_four_requests_and_resume_without_readmission() {
        use std::sync::{Arc, Mutex};
        use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
        use crate::hosted::{Execution, Snapshot, Workspace};
        let (files, fact_hash, criterion) = {
            let fs = crate::vfs::Scope::enter(&Snapshot::new());
            crate::vfs::create_dir_all("/project/.jazyk").unwrap();
            crate::vfs::write("/project/.jazyk/jazyk.toml",
                "version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Main'\ndocs = ['docs/**']\nsource = ['src/**', 'tests/**']\n").unwrap();
            let project = Project::load(Path::new("/project"));
            let mut store = crate::store::Store::load(&project.out);
            store.graph.entities.insert("ent:greeter".into(), crate::model::Entity {
                name: "Greeter".into(), ..Default::default()
            });
            store.graph.requirements.insert("req:greet".into(), crate::model::Requirement {
                statement: "The greeter says hello.".into(), entities: vec!["ent:greeter".into()], ..Default::default()
            });
            store.graph.criteria.insert("crit:greet".into(), crate::model::Criterion {
                requirement: "req:greet".into(), statement: "The greeting says hello.".into(),
                kind: "positive".into(), ..Default::default()
            });
            store.save();
            let mut ledger = crate::gen::Ledger::load(&project.out);
            ledger.medium = Some(crate::gen::Medium {
                form: "text".into(), produced: "written".into(), toolchain: String::new(), artifact: String::new()
            });
            ledger.save(&project.out);
            let mut control = crate::control::Control::load(&project, &project.out);
            control.generate = "auto".into();
            control.save(&project.out);
            let criterion = crate::gen::criteria_front_matter("req:greet", Some("crit:greet"),
                &store.graph.criteria["crit:greet"].statement)
                + "\nRead generated/greeting.txt. PASS only when it says Hello. FAIL otherwise.\n";
            (fs.snapshot(), crate::gen::fact_hash(&store, "ent:greeter"), criterion)
        };
        for interrupt in [false, true] {
            let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
            let observed = requests.clone();
            let fact_hash = fact_hash.clone();
            let criterion = criterion.clone();
            let config_hash = crate::model::hash_hex(std::str::from_utf8(&files[".jazyk/jazyk.toml"]).unwrap());
            let model = Arc::new(move |body: String| {
                let request: Value = serde_json::from_str(&body).unwrap();
                assert_eq!(request["_jazyk"]["purpose"], "inference");
                let mut seen = observed.lock().unwrap();
                seen.push(request);
                let round = seen.len();
                let (name, args) = match round {
                    1 => ("begin_generation", json!({"entity":"ent:greeter"})),
                    2 => ("update_document_set", json!({"baseHash":config_hash,"set":{"id":"ds_main","source":["src/**","generated/**","tests/**"]}})),
                    3..=26 => ("write_text_file", json!({"path":"generated/greeting.txt", "content":format!("Hello\nRevision {round}\n")})),
                    27 => ("write_text_file", json!({"path":"tests/greeting.md", "content":criterion})),
                    28 => ("record_generation", json!({"entity":"ent:greeter", "factHash":fact_hash,
                        "manifest":{"files":["generated/greeting.txt"], "tests":[{
                            "requirement":"req:greet", "criterion":"crit:greet", "kind":"llm",
                            "artifact":"tests/greeting.md", "name":"greeting", "run":"llm", "files":["generated/greeting.txt"]
                        }]}})),
                    29 => ("done", json!({"summary":"Recorded source and its criterion after incremental edits."})),
                    _ => panic!("successful ledger done must stop the worker"),
                };
                let reply = json!({"choices":[{"message":{"role":"assistant", "content":null,
                    "tool_calls":[{"id":format!("edit-{round}"),"type":"function",
                        "function":{"name":name,"arguments":args.to_string()}}]}}],
                    "usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}}).to_string();
                Box::pin(async move { Ok(reply) }) as std::pin::Pin<Box<dyn std::future::Future<Output=Result<String,String>> + Send>>
            });
            let checkpoint = Arc::new(Mutex::new(None::<(Value, Snapshot)>));
            let saved = checkpoint.clone();
            let injected = Arc::new(AtomicBool::new(false));
            let fail = injected.clone();
            let rounds = Arc::new(AtomicU32::new(0));
            let observed_rounds = rounds.clone();
            let sink = Arc::new(move |mut capsule: Value, files: Snapshot| {
                let next = capsule["sequence"].as_u64().unwrap() + 1;
                capsule["sequence"] = json!(next);
                let frame = capsule["frames"].as_array().and_then(|frames| frames.last());
                if let Some(rounds) = frame.and_then(|frame| frame["harness"]["rounds"].as_u64()) {
                    observed_rounds.fetch_max(rounds as u32, Ordering::SeqCst);
                }
                let crash = interrupt && frame.is_some_and(|frame|
                    frame["boundary"]["phase"] == "saved_model" && frame["harness"]["rounds"] == 25)
                    && !fail.swap(true, Ordering::SeqCst);
                *saved.lock().unwrap() = Some((capsule, files));
                Box::pin(async move {
                    if crash { Err("checkpoint-failed: fixture lost the save acknowledgement".into()) }
                    else { Ok(next) }
                }) as std::pin::Pin<Box<dyn std::future::Future<Output=Result<u64,String>> + Send>>
            });
            let mut execution = Execution { model:"scripted-generation".into(), transport_async:Some(model),
                checkpoint:Some(sink), ..Default::default() };
            let mut workspace = Workspace::new(files.clone()).unwrap();
            let body = json!({"kind":"gen","entities":["ent:greeter"]}).to_string();
            let mut result = workspace.request("POST", "/api/jobs", &body, &execution).unwrap();
            if interrupt {
                assert!(String::from_utf8_lossy(&result.body).contains("checkpoint-failed:"));
                assert_eq!(requests.lock().unwrap().len(), 25);
                let (capsule, protected) = checkpoint.lock().unwrap().clone().unwrap();
                assert_eq!(capsule["harness"]["rounds"], 25);
                assert_eq!(capsule["harness"]["model_requests"].as_object().unwrap().len(), 1,
                    "the capsule keeps only the current transition's request");
                let id = requests.lock().unwrap()[24]["_jazyk"]["requestId"].as_str().unwrap().to_string();
                let recorded = &capsule["harness"]["model_requests"][&id];
                let mut admitted = requests.lock().unwrap()[24].clone();
                admitted.as_object_mut().unwrap().remove("_jazyk");
                assert_eq!(recorded, &admitted, "the protected body owns its admitted identity");
                execution.resume = Some(capsule);
                workspace = Workspace::new(protected).unwrap();
                result = workspace.request("POST", "/api/jobs", &body, &execution).unwrap();
            }
            assert!(result.status < 400, "{}", String::from_utf8_lossy(&result.body));
            let result: Value = serde_json::from_slice(&result.body).unwrap();
            assert_eq!(result["report"]["regenerated"], 1, "{result}");
            let requests = requests.lock().unwrap();
            assert_eq!(requests.len(), 29, "a saved response must not repeat a model request");
            let identities: std::collections::BTreeSet<_> = requests.iter()
                .map(|request| request["_jazyk"]["requestId"].as_str().unwrap()).collect();
            assert_eq!(identities.len(), 29);
            assert_eq!(rounds.load(Ordering::SeqCst), 29, "resume must retain the original request count");
            let files = workspace.snapshot();
            assert!(files["generated/greeting.txt"].starts_with(b"Hello\n"));
            let _fs = crate::vfs::Scope::enter(&files);
            let project = Project::load(Path::new("/project"));
            let store = crate::store::Store::load(&project.out);
            assert!(crate::gen::pending(&store, &crate::gen::GenSettings::resolve(&project)).is_empty());
            assert_eq!(crate::gen::Ledger::load(&project.out).requirements["req:greet"].tests["crit:greet"].verdict, "none");
        }
    }

    #[test]
    fn only_an_ordinary_runtime_error_is_a_fatal_ledger_failure() {
        for stop in [Stop::EndTurn, Stop::MaxRounds, Stop::Cancelled, Stop::Suspended,
            Stop::Error("checkpoint-failed: disk".into()),
            Stop::Error("execution-uncertain: task".into())] {
            assert!(ledger_failure(&stop, 3).is_none(), "{stop:?}");
        }
        assert_eq!(ledger_failure(&Stop::Error("Local model request failed.".into()), 3),
            Some("the model call failed: Local model request failed.".into()));
    }
}

// The answer sessions a tab owes. A tab runs nothing in the background, so a reply
// recorded as `handling` waits, and a build runs one session per such answer before
// its first batch (docs/frontends/wasm.md#the-inbox): the `chat` serving, the
// handling prompt a native answer session gets, and the page's model. The outcome
// lands on the answer, `handled` or `failed`, the way the native handler records it.
// Answers one line per session for the build's reply.
#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub async fn run_answers(project: &Project, llm: &Llm, out: &Path, trace: &Trace) -> Vec<Value> {
    let mut results = Vec::new();
    for id in crate::answer::handling_ids(out) {
        let label = format!("answer {}", id);
        trace.event(TraceEvent::SessionStart {
            label: label.clone(),
            goals: Vec::new(),
            task: "answer".into(),
            target: id.clone(),
            doc: None,
            dirty: 0,
            sections: Vec::new(),
            stale: 0,
            proposals: 0,
        });
        let outcome = run_answer(project, llm, out, &id, &label, trace).await;
        if outcome.as_ref().err().is_some_and(|error| crate::session_recovery::is_interruption(error)) {
            break;
        }
        match &outcome {
            Ok(rounds) => trace.event(TraceEvent::SessionDone {
                label: label.clone(),
                goals: Vec::new(),
                staged: 0,
                rounds: *rounds,
                mode: "done".into(),
                summary: String::new(),
            }),
            Err(e) => trace.event(TraceEvent::SessionFailed {
                label: label.clone(),
                goals: Vec::new(),
                attempt: 1,
                error: e.clone(),
            }),
        }
        let recorded = outcome.as_ref().map(|_| ()).map_err(|e| e.clone());
        crate::answer::record_outcome(out, &id, &recorded);
        results.push(match outcome {
            Ok(_) => json!({"id": id, "status": "handled"}),
            Err(e) => json!({"id": id, "status": "failed", "error": e}),
        });
    }
    results
}

// One answer session: answers the rounds it took, or why it failed.
#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
async fn run_answer(
    project: &Project,
    llm: &Llm,
    out: &Path,
    id: &str,
    label: &str,
    trace: &Trace,
) -> Result<u32, String> {
    let prompt = crate::answer::handling_prompt(out, id)?;
    let server = McpServer::with_bridge(
        project.clone(),
        out.to_path_buf(),
        vec!["chat".to_string()],
        false,
        crate::mcp::BridgeFlags {
            ephemeral: true,
            ..Default::default()
        },
    );
    server
        .handle(
            "initialize",
            &json!({"protocolVersion": "2024-11-05", "clientInfo": {"name": "jazyk-wasm-answer"}}),
        )
        .map_err(|e| format!("serving: {}", e.1))?;
    let key = format!("answer:{}:{id}", out.display());
    let mut recovery = crate::session_recovery::McpRecovery::new(&server, key.clone())?;
    let mut model = harness::provider::DefaultModelHost::new(llm);
    let mut mcp_tools = crate::session_recovery::McpTools::plain(&server);
    let tools = InProcessRunner::tools_of(&server).map_err(|e| format!("serving: {}", e))?;
    let mut history: Vec<Value> = vec![json!({"role": "user", "content": prompt})];
    let batch = BatchRun {
        id: label.to_string(),
        goals: Vec::new(),
        executor: None,
    };
    let mut lt = LoopTrace::new(trace, &batch);
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
    let mut emit = |ev: AgentEvent| lt.on_event(ev);
    let never_cancelled = || trace.is_cancelled();
    let stop = harness::run_with_hosts(LoopArgs {
        llm,
        history: &mut history,
        tools: &tools,
        dispatch: &mut dispatch,
        emit: &mut emit,
        cancelled: &never_cancelled,
        max_rounds: crate::limits::agent_max_rounds(),
        label: label.to_string(),
        conversation: false,
        inputs: None,
        input_close: None,
    }, "", &mut model, &mut mcp_tools, &mut recovery).await;
    lt.finish();
    let rounds = crate::session_recovery::saved_usage(&key).map(|u| u.2).unwrap_or(lt.rounds);
    match stop {
        Stop::EndTurn => Ok(rounds),
        Stop::Suspended => Err("suspended".into()),
        Stop::Error(error) if crate::session_recovery::is_interruption(&error) => Err(error),
        other => Err(crate::runner::unlanded_reason(&other, rounds)),
    }
}
