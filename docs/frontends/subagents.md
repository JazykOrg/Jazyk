# Sub-agents

Jazyk's own agents can start sub-agents. A sub-agent is another agent run with its
own conversation. It works on a task its parent gives it, beside its parent, and its
final reply is delivered back to the parent. A sub-agent can start sub-agents of its
own, within the [limits](#limits).

Two agents offer sub-agents:

- The [embedded agent](./acp.md#the-embedded-agent) (`jazyk agent`), which the
  native [GUI chat](./gui.md#chat) runs.
- The conversations of the [browser engine](./wasm.md#chat) and the
  [hosted runtime](../compiler/hosted-runtime.md).

External agents (Claude Code, Codex, and other [ACP agents](./acp.md#agents)) use
their own sub-agents. Jazyk starts none for them. Their sub-agents call the same
Jazyk tools over the parent's MCP connection, and a serving holds
[several open batches](./mcp.md#the-control-plane-over-mcp), so they work in parallel without help.

Nothing is claimed. A sub-agent may do the same work as its parent or a sibling, or
different work. Commits are the only synchronization: the first commit wins and a
later one rebases or gets a [commit conflict](../compiler/graph.md#commit-conflicts)
([concurrent work](../compiler/control-plane.md#workers-and-open-work)).

## The agent tree

Every conversation is a node of one tree per top-level conversation:

- `parentId`: the conversation that started it. None for a conversation a person
  opened.
- `rootId`: the top-level conversation of its tree. A top-level conversation is its
  own root.
- `depth`: 0 for a top-level conversation, the parent's depth plus 1 otherwise.
- `task`: the task text the parent gave. None for a top-level conversation.
- `write`: what the sub-agent may change ([write scope](#write-scope)).

A sub-agent's id is its conversation's id. A sub-agent uses its parent's model and
agent.

## Agent tools

A Jazyk agent run lists five agent tools beside its serving's tools. They act only
on the caller's own descendants.

- `spawn_agent({task, title?, write?, wait?, wait_ms?})`: start a sub-agent. `task`
  says what to do and what to report. `write` is `none` (default), `all`, or a list
  of project-relative paths. Returns `agentId` and the sub-agent's state. With
  `wait: true` the call returns when the sub-agent's turn ends, with its result, or
  after `wait_ms` with its running state.
- `wait_agents({ids?, mode?, wait_ms?})`: wait for sub-agents. `ids` defaults to the
  caller's running children. `mode` is `all` (default) or `any`. `wait_ms` defaults
  to `AGENT_WAIT_DEFAULT_MS` and is capped at `AGENT_WAIT_MAX_MS`. Returns each named
  sub-agent's row, with its result once its turn ended.
- `agent_status({ids?})`: the rows of the caller's sub-agents, or of the named ids,
  without results.
- `message_agent({id, text})`: send a sub-agent a message. A running sub-agent
  receives it as [steering](./acp.md#steering-active-turns). An idle one starts a new
  turn with it, within the [limits](#limits).
- `cancel_agent({id})`: stop a sub-agent and its descendants.

A row carries `agentId`, `title`, `state` (`running`, `idle`, `failed`, or
`cancelled`), `depth`, `write`, and, when a call returns it, `result`
(`{status, text}`). A sub-agent at `AGENT_MAX_DEPTH` is not offered `spawn_agent`.

A refusal is a tool result, never a stopped turn: a limit, a write scope wider than
the caller's own, an id that is not the caller's descendant, or an empty task.

## Results

A sub-agent's result is the final reply of its turn: `completed` with the reply
text, `failed` with the error, or `cancelled`.

Delivery follows the GDK's sub-agent operation (`SubagentOperation` in
`goose-agent`):

- A finished turn is delivered to the parent as a message the model reads and the
  transcript shows: `Subagent <id> completed: <reply>`, `Subagent <id> failed:
  <error>`, or `Subagent <id> was cancelled before it finished.`
- The parent keeps working while its sub-agents run. A result is delivered when the
  parent ends its reply. A parent whose sub-agents still run does not end its turn:
  it waits for the next result, reads it, and continues. So a parent's turn ends only
  after every sub-agent it started has reported.
- A result a call returned (`spawn_agent` with `wait: true`, or `wait_agents`) is not
  delivered again. The operation records it as delivered, hidden from the model.
- A sub-agent's later turn has its own id, `<id>#<n>` for the n-th turn, so each
  turn reports once.

The delivered message is an input of the parent conversation with
`source: {agentId}`, so a transcript shows which sub-agent reported.

## Prompting a sub-agent

A person can open a sub-agent's conversation and write to it:

- Text to a running sub-agent steers its turn.
- A prompt to an idle sub-agent starts its next turn, `<id>#<n>`, with its own
  depth, write scope, and parent, within the [limits](#limits). A limit refuses the
  prompt and names the limit.
- The parent did not request that turn, so it does not wait for it. When the turn
  ends, its result is delivered to the parent as the same message, with
  `source: {agentId}`. A running parent receives it as
  [steering](./acp.md#steering-active-turns). An idle parent's conversation records
  it, and the parent's model reads it at its next turn.
- Stopping the sub-agent stops that turn and its descendants. The parent receives
  the `cancelled` result.

## Write scope

A sub-agent is read-only unless its parent grants more:

- `none`: the [read-only tools](../compiler/agent-runtime.md#models-and-tools), the
  agent tools, and the workflow skills. Other tools are not listed.
- A list of paths: also the tools whose path arguments (`path`, `paths`, `doc`,
  `file`) all fall under a listed file or directory. A tool with no path argument
  that is not read-only is refused.
- `all`: the parent's own tools.

A parent grants at most its own scope. The agent run enforces the scope on every
tool call of the sub-agent, before dispatch, and answers a refused call with a tool
result that names the scope.

## Limits

Registry constants, like the [graph limits](../compiler/graph.md#budgets-and-thresholds):

- `AGENT_MAX_DEPTH`: 3. A sub-agent at depth 3 has no `spawn_agent`.
- `AGENT_MAX_CHILDREN`: 4 running sub-agents per conversation.
- `AGENT_MAX_RUNNING`: 8 running sub-agents per agent process or engine.
- `AGENT_WAIT_DEFAULT_MS`: 60000. `AGENT_WAIT_MAX_MS`: 300000.

A spawn over a limit is refused as a tool result naming the limit. The agent waits
for a running sub-agent or does the work itself.

## Cancellation

- Stopping a conversation stops its running descendants. Each sub-agent's stop
  signal is derived from its parent's, so the stop cascades down the tree.
- On a stop, the parent's operation finalizes its cancellation: results that
  already finished are delivered, and the rest are recorded as cancelled.
- `cancel_agent` stops one sub-agent and its descendants. Its parent keeps running
  and receives the `cancelled` result.
- A turn that ends in an error or at its round budget stops its sub-agents the same
  way.
- Deleting a conversation deletes its sub-agents and their descendants, with their
  transcripts. Running ones stop first. A deleted sub-agent leaves the tree: its
  parent's agent tools no longer name it, and a turn the parent still waited for is
  delivered as failed.

## Where sub-agents run

Sub-agents run in process. Each runs the shared [agent runtime](../compiler/agent-runtime.md#sub-agents)
with its own conversation, its own serving, and so its own
[open batches](../compiler/control-plane.md#workers-and-open-work).

- The embedded agent runs each sub-agent turn on its own thread. The sub-agent opens
  its own connections to the session's MCP servers, so a `jazyk mcp chat` serving is
  started for it. Its updates travel on the root conversation's ACP session
  ([sub-agent updates](./acp.md#sub-agent-updates)), and the GUI shows each sub-agent
  as a conversation. A turn a person started can run while the root conversation is
  idle. Its updates travel on the root session all the same.
- The browser engine and the hosted runtime run sub-agents as asynchronous tasks on
  the workspace's one thread, interleaved with the parent. They mostly await the
  model, so they progress together, and their commits are serial by construction.
  Each sub-agent is a conversation of the engine, with its own `chat` serving.

## Checkpoints

A hosted sub-agent turn is checkpointed with its parent, in the same
[execution checkpoint](../compiler/agent-runtime.md#checkpoints):

- Each running sub-agent turn has its own frame. The parent's continuation records
  each turn it started, with the turn's opening text, and the result of each turn
  that ended.
- A suspension stops every running turn at its next durable boundary. The parent and
  its sub-agents suspend together. A suspended turn reports no result.
- A resumed parent continues each turn it still waits for: from the turn's frame, or
  from its opening when the turn saved no frame. A turn that had ended reports its
  recorded result. Saved model responses are reused, not requested again.
- Each result is delivered to the parent once, as in [results](#results).
- Stopping a suspended execution ends its sub-agent conversations as `cancelled`.
  Each parent conversation records the recorded results and `cancelled` for its
  other sub-agents, so its next turn does not wait for them.
- A turn the execution kept no record of, such as one lost with its process, is
  delivered as `Subagent <id> failed: ...`.

The browser engine takes no checkpoints. A tab that closes during a turn interrupts
the turn's sub-agents with it.

## Following agents

`GET /api/agent-tree` lists conversations, sub-agents, and running build sessions.
The `agents` event carries the same list whenever it changes
([agents](./gui.md#agents)). A sub-agent's transcript is an ordinary conversation:
`GET /api/chat/sessions/{id}` and the `chat.update` events serve it.
