//! Sub-agents of Jazyk's own agents: the agent tools, the tree, the limits, the
//! write scope, and result delivery through the GDK's `SubagentOperation`. A host
//! (the embedded agent, the browser and hosted conversations) supplies how one
//! sub-agent turn runs. Mirrors docs/frontends/subagents.md and
//! docs/compiler/agent-runtime.md#sub-agents.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use anyhow::Result;
use async_trait::async_trait;
use futures::{stream::FuturesUnordered, StreamExt};
use goose_agent::{
    operation::{self, ConversationEffect, Emitter, Operation, OperationResult, OperationStream},
    subagent::{SubagentOperation, SubagentOutcome, SubagentRunner, SubagentUpdate},
};
use goose_provider_types::conversation::{message::Message, Conversation};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::{
    runtime::{sleep_tick, Effect, Session},
    GenericTool, LocalFuture,
};

pub const TOOLS: [&str; 5] = [
    "spawn_agent",
    "wait_agents",
    "agent_status",
    "message_agent",
    "cancel_agent",
];

pub fn is_agent_tool(name: &str) -> bool {
    TOOLS.contains(&name)
}

// The GDK operation's name and notes (goose-agent `subagent.rs`). A delivered
// message carries the turn id under `delivered`; a cancellation message lists ids
// under `cancelled`.
const GDK_OPERATION: &str = "foreground_subagent";

static MESSAGES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

const TASK: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/payloads/goals/prompts/subagents/task.md.mustache"
));

/// What a sub-agent may change (docs/frontends/subagents.md#write-scope).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteScope {
    None,
    All,
    Paths(Vec<String>),
}

fn normal_path(path: &str) -> Option<String> {
    let path = path.trim().trim_start_matches("./").trim_end_matches('/');
    if path.is_empty()
        || path.starts_with('/')
        || path.split('/').any(|part| part == ".." || part.is_empty())
    {
        return None;
    }
    Some(path.to_string())
}

// The path arguments a scoped call is judged by.
const PATH_ARGUMENTS: [&str; 4] = ["path", "paths", "doc", "file"];

impl WriteScope {
    pub fn parse(value: &Value) -> Result<Self, String> {
        match value {
            Value::Null => Ok(Self::None),
            Value::String(s) if s == "none" => Ok(Self::None),
            Value::String(s) if s == "all" => Ok(Self::All),
            Value::Array(items) if !items.is_empty() => items
                .iter()
                .map(|item| {
                    item.as_str().and_then(normal_path).ok_or_else(|| {
                        format!("write paths must be project-relative paths, got {item}")
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Self::Paths),
            other => Err(format!(
                "write must be \"none\", \"all\", or a list of project-relative paths, got {other}"
            )),
        }
    }

    pub fn value(&self) -> Value {
        match self {
            Self::None => json!("none"),
            Self::All => json!("all"),
            Self::Paths(paths) => json!(paths),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Self::None => "none (read-only)".into(),
            Self::All => "all".into(),
            Self::Paths(paths) => format!("only {}", paths.join(", ")),
        }
    }

    fn covers(&self, path: &str) -> bool {
        match self {
            Self::None => false,
            Self::All => true,
            Self::Paths(paths) => normal_path(path).is_some_and(|path| {
                paths
                    .iter()
                    .any(|p| path == *p || path.starts_with(&format!("{p}/")))
            }),
        }
    }

    /// Whether a scope granted under `outer` stays inside it.
    pub fn within(&self, outer: &WriteScope) -> bool {
        match (self, outer) {
            (Self::None, _) | (_, Self::All) => true,
            (Self::All, _) => false,
            (Self::Paths(paths), outer) => paths.iter().all(|p| outer.covers(p)),
        }
    }

    /// Whether a call may dispatch under this scope.
    pub fn admits(&self, tool: &str, args: &Value) -> bool {
        if matches!(self, Self::All) || is_agent_tool(tool) || crate::tools::read_only(tool) {
            return true;
        }
        let Self::Paths(_) = self else {
            return false;
        };
        let mut named = Vec::new();
        for key in PATH_ARGUMENTS {
            match &args[key] {
                Value::String(path) => named.push(path.as_str()),
                Value::Array(items) => named.extend(items.iter().filter_map(Value::as_str)),
                _ => {}
            }
        }
        !named.is_empty() && named.iter().all(|path| self.covers(path))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    Running,
    Idle,
    Failed,
    Cancelled,
}
impl AgentState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Idle => "idle",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

pub fn outcome_value(outcome: &SubagentOutcome) -> Value {
    match outcome {
        SubagentOutcome::Completed(text) => json!({"status":"completed","text":text}),
        SubagentOutcome::Failed(text) => json!({"status":"failed","text":text}),
        SubagentOutcome::Cancelled => json!({"status":"cancelled","text":""}),
    }
}

/// One sub-agent in the tree.
#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub parent: String,
    pub root: String,
    pub depth: u32,
    pub title: String,
    pub task: String,
    pub write: WriteScope,
    pub state: AgentState,
    pub turns: u32,
    pub results: BTreeMap<String, SubagentOutcome>,
    pub inbox: Vec<Value>,
    pub cancel: CancellationToken,
}

impl Node {
    pub fn turn_id(&self, n: u32) -> String {
        if n <= 1 {
            self.id.clone()
        } else {
            format!("{}#{n}", self.id)
        }
    }

    pub fn last_result(&self) -> Option<&SubagentOutcome> {
        self.results.get(&self.turn_id(self.turns))
    }

    fn row(&self, result: bool) -> Value {
        let mut row = json!({"agentId": self.id, "title": self.title, "state": self.state.name(),
            "depth": self.depth, "parentId": self.parent, "write": self.write.value()});
        if result {
            if let Some(outcome) = self.last_result() {
                row["result"] = outcome_value(outcome);
            }
        }
        row
    }
}

/// The sub-agents of one agent process or engine. Top-level conversations are not
/// nodes: they are the roots the nodes hang from.
#[derive(Clone, Default)]
pub struct Tree(Arc<Mutex<BTreeMap<String, Node>>>);

impl Tree {
    pub fn get(&self, id: &str) -> Option<Node> {
        self.0.lock().unwrap().get(id).cloned()
    }

    pub fn nodes(&self) -> Vec<Node> {
        self.0.lock().unwrap().values().cloned().collect()
    }

    /// Add a sub-agent a host restored, unless the tree already holds it.
    pub fn adopt(&self, node: Node) {
        self.0
            .lock()
            .unwrap()
            .entry(node.id.clone())
            .or_insert(node);
    }

    fn insert(&self, node: Node) {
        self.0.lock().unwrap().insert(node.id.clone(), node);
    }

    fn update<T>(&self, id: &str, f: impl FnOnce(&mut Node) -> T) -> Option<T> {
        self.0.lock().unwrap().get_mut(id).map(f)
    }

    pub fn running(&self) -> usize {
        self.0
            .lock()
            .unwrap()
            .values()
            .filter(|n| n.state == AgentState::Running)
            .count()
    }

    pub fn running_children(&self, parent: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .values()
            .filter(|n| n.parent == parent && n.state == AgentState::Running)
            .count()
    }

    /// Whether `id` hangs below `ancestor`.
    pub fn descends(&self, id: &str, ancestor: &str) -> bool {
        let nodes = self.0.lock().unwrap();
        let mut current = nodes.get(id);
        while let Some(node) = current {
            if node.parent == ancestor {
                return true;
            }
            current = nodes.get(&node.parent);
        }
        false
    }

    /// Steering text for a running sub-agent, drained at its next safe boundary.
    pub fn take_inbox(&self, id: &str) -> Vec<Value> {
        self.update(id, |n| std::mem::take(&mut n.inbox))
            .unwrap_or_default()
    }

    /// Queue steering for a running sub-agent. False when it is not running.
    pub fn steer(&self, id: &str, input: Value) -> bool {
        self.update(id, |n| {
            let running = n.state == AgentState::Running && !n.cancel.is_cancelled();
            if running {
                n.inbox.push(input);
            }
            running
        })
        .unwrap_or(false)
    }

    /// Stop a sub-agent; its token's children stop with it.
    pub fn cancel(&self, id: &str) -> bool {
        self.update(id, |n| n.cancel.cancel()).is_some()
    }

    fn finish(&self, id: &str, turn: &str, outcome: SubagentOutcome) {
        self.update(id, |n| {
            n.state = match &outcome {
                SubagentOutcome::Completed(_) => AgentState::Idle,
                SubagentOutcome::Failed(_) => AgentState::Failed,
                SubagentOutcome::Cancelled => AgentState::Cancelled,
            };
            n.inbox.clear();
            n.results.insert(turn.to_string(), outcome);
        });
    }

    fn outcome(&self, turn: &str) -> Option<SubagentOutcome> {
        let id = turn.split('#').next().unwrap_or(turn);
        self.0
            .lock()
            .unwrap()
            .get(id)
            .and_then(|n| n.results.get(turn).cloned())
    }

    fn known(&self, turn: &str) -> bool {
        let id = turn.split('#').next().unwrap_or(turn);
        self.0.lock().unwrap().contains_key(id)
    }
}

/// One agent run: the caller of the agent tools.
#[derive(Clone)]
pub struct RunSpec {
    pub id: String,
    pub depth: u32,
    pub write: WriteScope,
    pub tree: Tree,
    pub cancel: CancellationToken,
}

impl RunSpec {
    /// A conversation a person opened: depth 0, every tool.
    pub fn root(id: &str, tree: Tree) -> Self {
        Self {
            id: id.to_string(),
            depth: 0,
            write: WriteScope::All,
            tree,
            cancel: CancellationToken::new(),
        }
    }

    /// The run of a sub-agent's turn, from its node.
    pub fn of(node: &Node, tree: Tree) -> Self {
        Self {
            id: node.id.clone(),
            depth: node.depth,
            write: node.write.clone(),
            tree,
            cancel: node.cancel.clone(),
        }
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

/// The new conversation a spawn asks the host for.
pub struct ChildRequest<'a> {
    pub parent: &'a RunSpec,
    pub root: &'a str,
    pub title: &'a str,
    pub task: &'a str,
    pub write: &'a WriteScope,
}

/// How a host makes sub-agents. `run_turn` answers the final reply of the turn:
/// the future runs on the parent's owner thread, interleaved with it, or awaits
/// work the host moved to its own thread.
pub trait ChildHost {
    fn create(&self, request: &ChildRequest<'_>) -> Result<String, String>;
    fn run_turn(&self, child: RunSpec, text: String) -> LocalFuture<'static, SubagentOutcome>;
    /// The root a top-level run's sub-agents hang from.
    fn root_of(&self, id: &str) -> String {
        id.to_string()
    }
}

// Sub-agent turns started by one run, polled beside it on its owner thread.
#[derive(Default)]
struct Pool {
    incoming: RefCell<Vec<LocalFuture<'static, ()>>>,
    tasks: RefCell<FuturesUnordered<LocalFuture<'static, ()>>>,
    waker: RefCell<Option<Waker>>,
}

impl Pool {
    fn spawn(&self, task: LocalFuture<'static, ()>) {
        self.incoming.borrow_mut().push(task);
        if let Some(waker) = self.waker.borrow_mut().take() {
            waker.wake();
        }
    }

    fn idle(&self) -> bool {
        self.incoming.borrow().is_empty() && self.tasks.borrow().is_empty()
    }

    fn poll_tasks(&self, cx: &mut Context<'_>) {
        *self.waker.borrow_mut() = Some(cx.waker().clone());
        loop {
            let incoming = std::mem::take(&mut *self.incoming.borrow_mut());
            let mut tasks = self.tasks.borrow_mut();
            tasks.extend(incoming);
            match tasks.poll_next_unpin(cx) {
                Poll::Ready(Some(())) => continue,
                Poll::Ready(None) | Poll::Pending => {
                    drop(tasks);
                    if self.incoming.borrow().is_empty() {
                        return;
                    }
                }
            }
        }
    }
}

/// Polls a run's sub-agent turns for as long as it is awaited.
pub(super) struct PoolRun<'a>(&'a Pool);
impl Future for PoolRun<'_> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.0.poll_tasks(cx);
        Poll::Pending
    }
}

/// Resolves once every sub-agent turn of the run ended.
pub(super) struct PoolDrain<'a>(&'a Pool);
impl Future for PoolDrain<'_> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.0.poll_tasks(cx);
        if self.0.idle() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// The agent tools of one run, and the sub-agent turns it started.
pub struct AgentRun {
    pub spec: RunSpec,
    host: Rc<dyn ChildHost>,
    pool: Pool,
}

fn refusal(text: String) -> Result<String, String> {
    Err(text)
}

fn limit(name: &str, value: impl std::fmt::Display) -> Result<String, String> {
    refusal(format!(
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/harness-agents/agent-limit-reached-6a0e8.md"
        )),
        limit = name,
        value = value
    ))
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> GenericTool {
    GenericTool {
        name: name.into(),
        description: description.into(),
        parameters: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    }
}

/// The agent tool definitions a run at `depth` lists.
pub fn definitions(depth: u32) -> Vec<GenericTool> {
    let ids = json!({"type":"array","items":{"type":"string"}});
    let wait_ms = json!({"type":"integer","minimum":0,"maximum":crate::limits::AGENT_WAIT_MAX_MS});
    let mut tools = Vec::new();
    if depth < crate::limits::AGENT_MAX_DEPTH {
        tools.push(tool(
            "spawn_agent",
            include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/harness-agents/spawn-agent-start-a-sub-agent-5b1e2.md")),
            json!({"task":{"type":"string"},"title":{"type":"string"},
                "write":{"anyOf":[{"type":"string","enum":["none","all"]},{"type":"array","items":{"type":"string"},"minItems":1}]},
                "wait":{"type":"boolean"},"wait_ms":wait_ms}),
            &["task"],
        ));
    }
    tools.push(tool(
        "wait_agents",
        include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/harness-agents/wait-agents-wait-for-sub-agents-7c4d1.md")),
        json!({"ids":ids,"mode":{"type":"string","enum":["all","any"]},"wait_ms":wait_ms}),
        &[],
    ));
    tools.push(tool(
        "agent_status",
        include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/harness-agents/agent-status-state-of-sub-agents-2a9f3.md")),
        json!({"ids":ids}),
        &[],
    ));
    tools.push(tool(
        "message_agent",
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/harness-agents/message-agent-send-a-sub-agent-8e6b0.md"
        )),
        json!({"id":{"type":"string"},"text":{"type":"string"}}),
        &["id", "text"],
    ));
    tools.push(tool(
        "cancel_agent",
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/harness-agents/cancel-agent-stop-a-sub-agent-4f7a6.md"
        )),
        json!({"id":{"type":"string"}}),
        &["id"],
    ));
    tools
}

impl AgentRun {
    pub fn new(spec: RunSpec, host: Rc<dyn ChildHost>) -> Self {
        Self {
            spec,
            host,
            pool: Pool::default(),
        }
    }

    pub(super) fn pool_run(&self) -> PoolRun<'_> {
        PoolRun(&self.pool)
    }

    pub(super) fn pool_drain(&self) -> PoolDrain<'_> {
        PoolDrain(&self.pool)
    }

    /// The catalog this run lists: its serving's tools within its write scope, and
    /// the agent tools.
    pub fn catalog(&self, tools: &mut Vec<GenericTool>) {
        if self.spec.write == WriteScope::None {
            tools.retain(|t| crate::tools::read_only(&t.name));
        }
        tools.retain(|t| !is_agent_tool(&t.name));
        tools.extend(definitions(self.spec.depth));
    }

    /// The refusal for a call outside this run's write scope.
    pub fn refuse(&self, tool: &str, args: &Value) -> Option<String> {
        (!self.spec.write.admits(tool, args)).then(|| {
            format!(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/harness-agents/write-scope-refused-9d2c4.md"
                )),
                tool = tool,
                scope = self.spec.write.describe()
            )
        })
    }

    fn own(&self, id: &str) -> Result<Node, String> {
        match self.spec.tree.get(id) {
            Some(node) if self.spec.tree.descends(id, &self.spec.id) => Ok(node),
            _ => Err(format!(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/harness-agents/not-your-sub-agent-1c8e5.md"
                )),
                id = id
            )),
        }
    }

    fn start_turn(&self, node: &Node, turn: String, text: String) {
        let spec = RunSpec::of(node, self.spec.tree.clone());
        let tree = self.spec.tree.clone();
        let id = node.id.clone();
        let run = self.host.run_turn(spec, text);
        self.pool.spawn(Box::pin(async move {
            let outcome = run.await;
            tree.finish(&id, &turn, outcome);
        }));
    }

    fn startable(&self) -> Result<(), String> {
        if self.spec.tree.running_children(&self.spec.id) >= crate::limits::AGENT_MAX_CHILDREN {
            return limit("AGENT_MAX_CHILDREN", crate::limits::AGENT_MAX_CHILDREN).map(|_| ());
        }
        if self.spec.tree.running() >= crate::limits::AGENT_MAX_RUNNING {
            return limit("AGENT_MAX_RUNNING", crate::limits::AGENT_MAX_RUNNING).map(|_| ());
        }
        Ok(())
    }

    /// Answer one agent tool call (docs/frontends/subagents.md#agent-tools).
    pub async fn call(&self, name: &str, args: &Value) -> Result<String, String> {
        let reply = match name {
            "spawn_agent" => self.spawn(args).await?,
            "wait_agents" => {
                let ids: Vec<String> = match args["ids"].as_array() {
                    Some(ids) => ids
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect(),
                    None => self
                        .spec
                        .tree
                        .nodes()
                        .into_iter()
                        .filter(|n| n.parent == self.spec.id && n.state == AgentState::Running)
                        .map(|n| n.id)
                        .collect(),
                };
                for id in &ids {
                    self.own(id)?;
                }
                self.wait(&ids, args["mode"] == "any", wait_bound(args))
                    .await
            }
            "agent_status" => {
                let rows = match args["ids"].as_array() {
                    Some(ids) => ids
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|id| self.own(id).map(|n| n.row(false)))
                        .collect::<Result<Vec<_>, _>>()?,
                    None => self
                        .spec
                        .tree
                        .nodes()
                        .into_iter()
                        .filter(|n| self.spec.tree.descends(&n.id, &self.spec.id))
                        .map(|n| n.row(false))
                        .collect(),
                };
                json!({"agents": rows})
            }
            "message_agent" => {
                let node = self.own(args["id"].as_str().unwrap_or_default())?;
                let text = args["text"].as_str().unwrap_or_default().trim();
                if text.is_empty() {
                    return Err("text is required: the message to send".into());
                }
                let input = json!({"text": text,
                    "messageId": format!("agent-{}-{}", self.spec.id, MESSAGES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)),
                    "source": {"agentId": self.spec.id}});
                if self.spec.tree.steer(&node.id, input) {
                    json!({"agentId": node.id, "state": "running", "delivered": "steering"})
                } else {
                    self.startable()?;
                    let cancel = self.spec.cancel.child_token();
                    let node = self
                        .spec
                        .tree
                        .update(&node.id, |n| {
                            n.turns += 1;
                            n.state = AgentState::Running;
                            n.cancel = cancel;
                            n.clone()
                        })
                        .ok_or("the sub-agent left the tree")?;
                    let turn = node.turn_id(node.turns);
                    self.start_turn(&node, turn.clone(), text.to_string());
                    json!({"agentId": node.id, "state": "running", "started": turn})
                }
            }
            "cancel_agent" => {
                let node = self.own(args["id"].as_str().unwrap_or_default())?;
                self.spec.tree.cancel(&node.id);
                json!({"agentId": node.id, "cancelled": true})
            }
            other => return Err(format!("unknown agent tool `{other}`")),
        };
        Ok(reply.to_string())
    }

    async fn spawn(&self, args: &Value) -> Result<Value, String> {
        let task = args["task"].as_str().unwrap_or_default().trim();
        if task.is_empty() {
            return Err("task is required: what the sub-agent does and reports".into());
        }
        if self.spec.depth >= crate::limits::AGENT_MAX_DEPTH {
            return limit("AGENT_MAX_DEPTH", crate::limits::AGENT_MAX_DEPTH).map(|_| json!({}));
        }
        self.startable()?;
        let write = WriteScope::parse(&args["write"])?;
        if !write.within(&self.spec.write) {
            return Err(format!(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/harness-agents/agent-scope-wider-than-own-3b5d7.md"
                )),
                scope = self.spec.write.describe()
            ));
        }
        let title = args["title"]
            .as_str()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| task.lines().next().unwrap_or(task));
        let title = crate::llm::truncate(title, 60);
        let root = match self.spec.tree.get(&self.spec.id) {
            Some(node) => node.root,
            None => self.host.root_of(&self.spec.id),
        };
        let id = self.host.create(&ChildRequest {
            parent: &self.spec,
            root: &root,
            title: &title,
            task,
            write: &write,
        })?;
        let node = Node {
            id: id.clone(),
            parent: self.spec.id.clone(),
            root,
            depth: self.spec.depth + 1,
            title,
            task: task.to_string(),
            write: write.clone(),
            state: AgentState::Running,
            turns: 1,
            results: BTreeMap::new(),
            inbox: Vec::new(),
            cancel: self.spec.cancel.child_token(),
        };
        self.spec.tree.insert(node.clone());
        let text = crate::payload::fill(
            TASK,
            &[
                ("PARENT", &self.spec.id),
                ("TASK", task),
                ("WRITE", &write.describe()),
            ],
        );
        self.start_turn(&node, id.clone(), text);
        let mut reply = json!({"agentId": id, "title": node.title, "state": "running",
            "depth": node.depth, "write": write.value(), "started": id});
        if args["wait"] == true {
            let waited = self
                .wait(std::slice::from_ref(&id), false, wait_bound(args))
                .await;
            reply["state"] = waited["agents"][0]["state"].clone();
            if let Some(result) = waited["agents"][0].get("result") {
                reply["result"] = result.clone();
            }
            reply["returned"] = waited["returned"].clone();
        }
        Ok(reply)
    }

    async fn wait(&self, ids: &[String], any: bool, bound: u64) -> Value {
        let started = crate::sys::Instant::now();
        loop {
            let nodes: Vec<Node> = ids.iter().filter_map(|id| self.spec.tree.get(id)).collect();
            let done = |n: &Node| n.state != AgentState::Running && n.last_result().is_some();
            let finished = if any {
                nodes.iter().any(done) || nodes.is_empty()
            } else {
                nodes.iter().all(done)
            };
            if finished || self.spec.cancelled() || started.elapsed().as_millis() as u64 >= bound {
                let returned: Vec<String> = nodes
                    .iter()
                    .filter(|n| done(n))
                    .map(|n| n.turn_id(n.turns))
                    .collect();
                return json!({"agents": nodes.iter().map(|n| n.row(done(n))).collect::<Vec<_>>(),
                    "returned": returned});
            }
            sleep_tick().await;
        }
    }
}

fn wait_bound(args: &Value) -> u64 {
    args["wait_ms"]
        .as_u64()
        .unwrap_or(crate::limits::AGENT_WAIT_DEFAULT_MS)
        .min(crate::limits::AGENT_WAIT_MAX_MS)
}

// ---- delivery through the GDK operation ----

fn agent_tool_texts(message: &Message) -> impl Iterator<Item = Value> + '_ {
    message
        .content
        .iter()
        .filter_map(|content| content.as_tool_response_text())
        .filter(|text| text.contains("\"started\"") || text.contains("\"returned\""))
        .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
}

/// The turns whose results tool calls in this conversation already returned.
fn returned(conversation: &Conversation) -> BTreeSet<String> {
    conversation
        .messages()
        .iter()
        .flat_map(agent_tool_texts)
        .flat_map(|value| {
            value["returned"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn delivered_ids(message: &Message) -> Vec<String> {
    let delivered = message
        .metadata
        .operation_note(GDK_OPERATION, "delivered")
        .and_then(Value::as_str)
        .map(str::to_string);
    let cancelled = message
        .metadata
        .operation_note(GDK_OPERATION, "cancelled")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string);
    delivered.into_iter().chain(cancelled).collect()
}

fn requested(message: &Message, returned: &BTreeSet<String>) -> Vec<String> {
    agent_tool_texts(message)
        .filter_map(|value| value["started"].as_str().map(str::to_string))
        .filter(|turn| !returned.contains(turn))
        .collect()
}

/// The sub-agent turns that still owe this conversation a result: requested by a
/// tool result and neither delivered, cancelled, nor returned. The same rule as
/// the GDK operation's pending set.
pub fn owed(conversation: &Conversation) -> Vec<String> {
    let returned = returned(conversation);
    let mut pending: Vec<String> = Vec::new();
    for message in conversation.messages() {
        for id in delivered_ids(message) {
            pending.retain(|p| *p != id);
        }
        for id in requested(message, &returned) {
            if !pending.contains(&id) {
                pending.push(id);
            }
        }
    }
    pending
}

/// The transcript input a delivered result becomes, with `source: {agentId}`.
pub fn delivered_input(message: &Message) -> Option<Value> {
    if !message.is_agent_visible() {
        return None;
    }
    let ids = delivered_ids(message);
    let first = ids.first()?;
    let agent = first.split('#').next().unwrap_or(first);
    Some(json!({
        "messageId": message.id.clone().unwrap_or_else(|| format!("agent-result-{first}")),
        "text": message.as_concat_text(),
        "source": {"agentId": agent, "turns": ids},
    }))
}

struct Streams(Tree);

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl SubagentRunner<Session> for Streams {
    fn requested_subagent_session_ids(&self, session: &Session, message: &Message) -> Vec<String> {
        requested(message, &returned(&session.conversation))
    }

    // The turn already runs: spawn_agent and message_agent start it. The stream
    // reports its recorded outcome. A turn this tree never ran (its process is gone)
    // fails (docs/frontends/subagents.md#where-sub-agents-run).
    async fn start(
        &self,
        _: &Session,
        turn: &str,
        _: CancellationToken,
    ) -> OperationStream<'static, SubagentUpdate> {
        let tree = self.0.clone();
        let turn = turn.to_string();
        Box::pin(futures::stream::unfold(Some(()), move |state| {
            let tree = tree.clone();
            let turn = turn.clone();
            async move {
                state?;
                loop {
                    if let Some(outcome) = tree.outcome(&turn) {
                        return Some((SubagentUpdate::Finished(outcome), None));
                    }
                    if !tree.known(&turn) {
                        return Some((
                            SubagentUpdate::Finished(SubagentOutcome::Failed(
                                "the sub-agent turn did not survive its process".into(),
                            )),
                            None,
                        ));
                    }
                    sleep_tick().await;
                }
            }
        }))
    }
}

/// The run's GDK sub-agent operation, held across passes. A result a tool call
/// already returned is recorded hidden from the model and the transcript.
pub(super) struct AgentsOperation {
    inner: SubagentOperation<Session>,
}

impl AgentsOperation {
    pub(super) fn new(tree: Tree) -> Self {
        Self {
            inner: SubagentOperation::new(Arc::new(Streams(tree))).with_max_concurrency(
                std::num::NonZeroUsize::new(crate::limits::AGENT_MAX_RUNNING)
                    .unwrap_or(std::num::NonZeroUsize::MIN),
            ),
        }
    }

    pub(super) async fn finalize(&self, session: &Session, emit: &Emitter) -> Vec<Message> {
        <SubagentOperation<Session> as Operation<Session, Effect>>::finalize_cancellation(
            &self.inner,
            session,
            &session.conversation,
            emit,
        )
        .await
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::Conversation(ConversationEffect::AppendMessage(message)) => Some(message),
            _ => None,
        })
        .collect()
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Operation<Session, Effect> for AgentsOperation {
    fn name(&self) -> &'static str {
        "jazyk_subagents"
    }

    async fn run(
        &self,
        session: &Session,
        conversation: &Conversation,
        emit: &Emitter,
    ) -> Result<OperationResult<Effect>> {
        let returned = returned(conversation);
        let mut conversation = conversation.clone();
        let mut hidden = Vec::new();
        loop {
            let result = self.inner.run(session, &conversation, emit).await?;
            let OperationResult::Applied(mut step) = result else {
                return if hidden.is_empty() {
                    operation::not_applicable()
                } else {
                    operation::applied(hidden)
                };
            };
            let mut visible = false;
            for effect in &mut step.effects {
                if let Effect::Conversation(ConversationEffect::AppendMessage(message)) = effect {
                    if delivered_ids(message)
                        .iter()
                        .all(|id| returned.contains(id))
                    {
                        message.metadata.user_visible = false;
                        message.metadata.agent_visible = false;
                    } else {
                        visible = true;
                    }
                    conversation.push(message.clone());
                }
            }
            hidden.append(&mut step.effects);
            if visible {
                return operation::applied(hidden);
            }
        }
    }
}

/// The final reply of a finished run's history: its last assistant text.
pub fn final_reply(history: &[Value]) -> String {
    history
        .iter()
        .rev()
        .filter(|m| m["role"] == "assistant")
        .find_map(|m| m["content"].as_str().filter(|t| !t.trim().is_empty()))
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// A run's stop as a sub-agent outcome.
pub fn outcome_of(stop: &super::Stop, history: &[Value]) -> SubagentOutcome {
    match stop {
        super::Stop::EndTurn => SubagentOutcome::Completed(final_reply(history)),
        super::Stop::Cancelled => SubagentOutcome::Cancelled,
        super::Stop::MaxRounds => SubagentOutcome::Failed(format!(
            "the sub-agent reached its round budget; its last reply: {}",
            final_reply(history)
        )),
        super::Stop::Suspended => SubagentOutcome::Failed("the sub-agent was suspended".into()),
        super::Stop::Error(error) => SubagentOutcome::Failed(error.clone()),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "agents_tests.rs"]
mod tests;
