# ACP

Jazyk speaks the [Agent Client Protocol](https://agentclientprotocol.com) (ACP) and
sits between ACP clients and ACP agents. An ACP client is an editor or IDE (Zed,
JetBrains, or jazyk's own [GUI](./gui.md)). An ACP agent is a coding agent (OpenCode,
Codex through `codex-acp`, or jazyk's [embedded agent](#the-embedded-agent)).

ACP is the single AI path. Compilation, [binding](../consumers/bind.md),
[generation](../consumers/gen.md), verification judgment, and chat all run as ACP
sessions against the configured agent. Jazyk itself never calls a model endpoint
outside the embedded agent. [MCP](./mcp.md) stays as the tool delivery mechanism into
sessions, and as the backwards-compatible serving for agents that connect on their own.

## Roles

Jazyk takes both protocol roles, depending on the direction:

- Client of the downstream agent. For automated work and for the GUI's chat pane,
  jazyk spawns the configured agent as a subprocess and drives it: it creates
  sessions, sends prompts, and consumes the update stream. ACP agents cannot create
  sessions or start turns, so the automated path must own the client side.
- Agent toward the IDE. `jazyk acp` is a stdio process an IDE spawns like any other
  ACP agent. It proxies to the downstream agent and adds jazyk on the way through:
  tool injection, doc edit translation, build status. See [the IDE proxy](#the-ide-proxy).

One process that runs builds holds one connection to one downstream agent, with many
concurrent sessions on it. The [control plane](../compiler/reconciler.md#the-control-plane)
arbitrates between processes as it does today: the build lease and the per-task leases
make a CLI run beside a GUI harmless.

## Agents

The downstream agent is a named profile in the [`[acp]` settings](../compiler/project-settings.md#acp).
The default profile is `embedded`. Agent choice is configuration only; nothing in
jazyk is specific to any agent.

A profile names a command, arguments, extra environment, and whether jazyk must serve
file tools to it (`serve_files`, for agents that bring no editor of their own).

## The embedded agent

`jazyk agent` runs a minimal, generic ACP agent over stdio. It exists so jazyk works
with no external agent installed, and it is deliberately ignorant of jazyk:

- It answers `initialize` with protocol version 1 and stdio MCP support.
- On `session/new` it spawns the listed MCP servers and collects their tools.
- On `session/prompt` it runs a plain agentic loop against the configured
  OpenAI-compatible endpoint ([LLM settings](../compiler/project-settings.md#llm)):
  send the messages and the MCP tools, dispatch the calls the model makes, append the
  results, repeat until the model stops calling tools. It streams thought chunks,
  message chunks, one `tool_call` and `tool_call_update` per MCP call, and token
  usage. A prose reply in a turn that already made tool calls gets one nudge before
  the turn ends: weak models forget they are mid-task more often than they finish
  silently, and a purely conversational answer still ends the turn immediately.
- `session/cancel` stops the loop at the next round.
- `session/close` (advertised in its capabilities) tears the session down: each MCP
  server's input closes and the agent waits for it to exit before answering. An
  ephemeral jazyk serving runs its implicit finish on that end of input, so closing
  a worker session is what lands a turn whose agent forgot the finishing call.

The codecs live here: `native` (OpenAI-style `tools` and `tool_calls`, with the
calls for one step batched into a single reply) and `text` (tools described in the
system prompt, one JSON action object per reply, because small models cannot
reliably emit several). The agent probes on the first round: an endpoint that
rejects the `tools` parameter, or a model that answers prose without calls,
downgrades the run to `text`, sticky until it ends. The endpoint fallbacks ride
along too: streaming when the endpoint demands it, dropped `temperature`, stripped
reasoning fields ([LLM settings](../compiler/project-settings.md#llm)). A reasoning
model's reasoning is appended back into the history unchanged, so later rounds see
the reasoning behind earlier calls. No jazyk prompting, no jazyk tool knowledge, no
shortcut into the store. The same session against OpenCode or the embedded agent
carries the same prompt and the same injected tools, which is what makes the embedded
agent a faithful test double for the whole path.

## Sessions

Three session kinds, all the same protocol:

- Worker sessions: created by jazyk for one unit of automated work. One session per
  [work item](../compiler/turns.md), so a turn keeps its fresh, focused context and a
  retry starts clean. Parallel waves run as concurrent sessions on the one connection,
  bounded by `JAZYK_MAX_CONCURRENCY`.
- Chat sessions: created by a user in the [GUI chat pane](./gui.md#chat) or from an
  IDE through the proxy. The agent gets the `chat` toolset: graph reads and writes,
  the task lifecycle, and the [dual-write requirement tools](#dual-write-tools).
- Follow sessions: read-only mirrors of worker sessions, so a person can watch
  automated work as it happens. In the GUI they appear in the chat pane beside chat
  sessions. Toward IDEs they are served through [session list mirroring](#mirroring-into-ides).

## Worker sessions

The automated path. For each work item the runner:

1. Creates a session whose `mcpServers` list one entry: `jazyk mcp` with the task's
   toolsets and flags (see [MCP into sessions](./mcp.md#mcp-into-acp-sessions)).
2. Prompts with a fixed, agent-neutral instruction: begin the named task with the
   lifecycle tool, follow the returned package, finish with the finishing tool,
   repair what a rejection names. The task's real prompt (the
   [context pack](../compiler/context.md) and the task contract) rides inside the
   `begin_*` reply, the same package every consumer gets. One prompt source, no
   duplication.
3. Consumes the update stream and translates it into
   [trace events](../compiler/turns.md#trace-events): message and thought chunks
   become model text, `tool_call` and `tool_call_update` become tool rows, usage
   updates accumulate into the token count. The trace, the transcript, and the GUI
   panels do not care which agent ran the turn.
4. Closes the session and waits for the teardown, so an agent that ended its turn
   with staged work but no finishing call still lands it: the serving's implicit
   finish runs on the teardown, under the same gates the budget path uses.
5. Reads success from the store, never from the agent's word: the commit happened
   inside the MCP serving under its own gates, so the runner attributes the journal
   entries between the session's start and end generations to the work item, and a
   compilation item must have left the queue. A turn whose task did not land is a
   failed turn, whatever the agent said. A retry is a fresh session; its claim on the
   same task is re-entrant, so its own earlier attempt never blocks it.

A session that goes silent is cancelled after an idle timeout (`JAZYK_ACP_IDLE_TIMEOUT`,
default 600 seconds). Cancellation follows the protocol: pending permission requests
are answered `cancelled`, then `session/cancel` goes out. The lease TTLs bound what a
dead agent can hold either way.

## Chat sessions

A chat session is an open conversation with the agent about the project. The injected
`chat` serving carries the read tools, the write tools, the task lifecycle, and the
project tools, so "tighten this requirement and recompile" is a sentence, not a
workflow.

### Dual-write tools

A requirement lives in the prose; the graph carries its compiled form and a verbatim
quote. A chat edit must move both or neither:

- `revise_requirement` takes the requirement id, the new prose, and optionally the
  new `ears`. It locates the old quote in the document, stages the prose edit and the
  requirement update as one changeset, and commits them atomically. The document's
  stored content hash updates in the same commit, so the edit does not dirty the
  document it just reconciled.
- The prose edit surfaces to the ACP client as a file write plus a diff on the tool
  call, so an IDE shows it in the buffer and the review UI. See
  [doc edit delegation](#doc-edit-delegation).
- Direct graph writes without a prose edit are not in the `chat` toolset. That path
  remains only in `jazyk mcp graph --write`, the debugging surface.

### Project tools

Setup and configuration are chat tools too, routed through the same edit delegation:

- `init_project`: scaffold `jazyk.toml` and the starter layout, what
  [`jazyk init`](./cli.md#jazyk-init) writes.
- `update_project_settings`: typed edits to `jazyk.toml` (workflow modes, docs glob,
  lint rules, the `[acp]` profile), rendered as minimal edits, never a whole-file
  rewrite.

### Slash commands

Chat sessions advertise commands through `available_commands_update`: `/compile`,
`/generate`, `/verify`, `/status`, `/release`. ACP has no invoke method for commands;
they arrive as prompt text. Jazyk matches the prefix before the prompt reaches the
agent: a matched command runs the real work (the same path as the CLI command of that
name) and streams its progress into the open turn, then ends the turn. Unmatched
prompts go to the agent as conversation.

## Plans

Build progress is an ACP plan. The runner publishes one plan entry per scheduled work
item ("reconcile docs/cli.md", "review ent:cart", "generate ent:order", "verify
req:order-3") and flips each entry `pending` → `in_progress` → `completed` as the
build advances. Plan updates replace the whole list, per the protocol. The GUI renders
the plan as a live checklist; an IDE that triggered the build through `/compile` sees
the same plan inside its turn.

## Permissions

Two policies, chosen per session kind:

- Worker sessions answer permission requests by rule: reads and jazyk tool calls are
  allowed, anything touching files outside the project or the deliverable is
  rejected. Automated work never blocks on a human.
- Chat sessions forward permission requests to the user: the GUI shows them in the
  pane, the proxy passes them through to the IDE. An unanswered request cancels with
  the turn.

## The IDE proxy

`jazyk acp` is the process an IDE registers as its "Jazyk" agent. Upstream it serves
the protocol on stdio; downstream it drives the configured agent. In between:

- `initialize` passes through, recording the IDE's capabilities (file system support
  decides whether [doc edit delegation](#doc-edit-delegation) is available).
- `session/new` gains the injected `jazyk mcp chat` entry in `mcpServers` before it
  reaches the agent.
- Updates pass through verbatim. Jazyk decorates where it has something to add, and
  its own additions use namespaced tool call ids so they never collide with the
  agent's.
- Slash commands are intercepted as in the GUI: `/compile` runs the build through
  worker sessions and streams synthetic tool calls and the [plan](#plans) into the
  open IDE turn.
- Outside a jazyk project (no `jazyk.toml` above the session's `cwd`), the proxy is a
  transparent passthrough plus one advertised command, `/jazyk-init`, which scaffolds
  a project and switches the session to the full bridge. The IDE's global agent entry
  works everywhere; jazyk lights up only where a project exists.

### Doc edit delegation

When a chat tool edits a document or `jazyk.toml`, the write should land where the
user is looking: the IDE's unsaved buffer, not just the disk. The injected MCP serving
is told to delegate edits (`--edit-sink`); each edit travels to the proxy, which
issues the file write upstream and attaches the diff to a tool call update. When the
IDE lacks file system support, or nothing listens on the sink, the serving writes the
disk directly. The graph commit is the same either way.

### Mirroring into IDEs

An agent cannot hand its client a new session, so background work cannot open a
window in the IDE. What the protocol does allow:

- The proxy advertises `loadSession` and session listing, and lists worker runs as
  read-only sessions with descriptive titles ("compile docs/cli.md"). An IDE with a
  session picker attaches through the standard `session/load`, which replays the full
  history (tool calls included) and then streams the live tail.
- `_jazyk/session_list_changed` is a custom notification (underscore-namespaced, so
  clients that do not know it ignore it, per the protocol's extensibility rules)
  telling a capable client to refresh its session list. The GUI honors it; plain IDEs
  lose nothing.
- Background builds additionally push the [plan](#plans) and a session info update
  into the most recently active jazyk session, and only that one. Rendering of
  updates outside a turn varies by IDE; the GUI always renders them.

### LSP and the proxy

The [LSP](./lsp.md) stays read-only and never compiles. What it knows (stale
documents, open diagnostics, pending work) reaches the IDE's chat surface through the
proxy instead: on session start and on every
[control plane](../compiler/reconciler.md#the-control-plane) or queue change, the
proxy refreshes the advertised commands and the pending-work plan. The queue and the
leases are files, so the proxy reads them the way every other frontend does, no new
channel.

## Registration

Editors register ACP agents globally, not per project (JetBrains in
`~/.jetbrains/acp.json`, Zed in its global `settings.json`).
`jazyk acp install --ide <jetbrains|zed>` merges a `Jazyk` entry pointing at
`jazyk acp` into the named editor's registry, never overwriting other entries.
[`jazyk init`](./cli.md#jazyk-init) offers the same step. Per-project behavior comes
from the protocol: every session names its `cwd`, and the proxy resolves the project
from there, so one global entry serves every project and stays inert outside them.

## Protocol versions

The internals are modeled on protocol version 2 even while the wire speaks version 1:

- Tools reach agents only through injected MCP servers. Jazyk never relies on the
  protocol's file system or terminal methods toward agents; version 2 removes them.
- Turn progress is consumed from the update stream, not the pending prompt response.
- Tool calls are treated as upserts by id, and unknown enum values are tolerated.

The wire version is negotiated per connection in `initialize`, in both roles. Today's
agents and IDEs answer version 1; version 2 engages per peer as peers ship it. The
one version 1 convenience in use is the upstream file write in
[doc edit delegation](#doc-edit-delegation), which degrades to a direct disk write.

## Configuration

The `[acp]` section of `jazyk.toml`, the global `~/.jazyk/config.toml`, the
`JAZYK_ACP_AGENT` environment variable, and the `--agent` flag resolve per field like
the [LLM settings](../compiler/project-settings.md#llm) do. See
[project settings](../compiler/project-settings.md#acp).
