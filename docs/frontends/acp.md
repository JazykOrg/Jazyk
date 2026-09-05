# ACP

Jazyk speaks the [Agent Client Protocol](https://agentclientprotocol.com) (ACP) and
sits between ACP clients and ACP agents. An ACP client is an editor or IDE (Zed,
JetBrains, or jazyk's own [GUI](./gui.md)). An ACP agent is a coding agent (OpenCode,
Claude Code, Codex through `codex-acp`, or jazyk's [embedded agent](#the-embedded-agent)).

ACP is the single AI path. Compilation, [binding](../consumers/bind.md),
[generation](../consumers/gen.md), verification judgment, and chat all run as ACP
sessions against a configured agent: the `[acp]` profile, or the
[executor](#executors) an override names for a goal kind or a goal class. Jazyk itself
never calls a model endpoint outside the embedded agent. [MCP](./mcp.md) stays as the
tool delivery mechanism into sessions, and as the serving for agents that connect on
their own.

## Roles

Jazyk takes both protocol roles, depending on the direction:

- Client of the downstream agent. For automated work and for the GUI's chat pane,
  jazyk spawns the configured agent as a subprocess and drives it: it creates
  sessions, sends prompts, and consumes the update stream. ACP agents cannot create
  sessions or send prompts, so the automated path must own the client side.
- Agent toward the IDE. `jazyk acp` is a stdio process an IDE spawns like any other
  ACP agent. It proxies to the downstream agent and adds jazyk on the way through:
  tool injection, doc edit translation, build status. See [the IDE proxy](#the-ide-proxy).

One process that runs builds holds one connection per downstream agent it drives, with
several sessions open on it: one worker session at a time (compilation is
sequential), chat and follow sessions beside it. The
[control plane](../compiler/control-plane.md) arbitrates between processes: the build
lease and the per-batch leases make a CLI run beside a GUI harmless.

## Agents

The downstream agent is a named profile in the [`[acp]` settings](../compiler/project-settings.md#acp).
The default profile is `embedded`. Agent choice is configuration only; nothing in
jazyk is specific to any agent.

A profile names a command, arguments, extra environment, and whether jazyk must serve
file tools to it (`serve_files`, for agents that bring no editor of their own).

### Claude Code

Claude Code runs as an ACP agent through Zed's adapter. The profile:

```toml
[acp.agents.claude]
command = "npx"
args = ["--yes", "@zed-industries/claude-code-acp"]
```

What the adapter does that a session must account for:

- Claude Code refuses to start inside another Claude Code session. Every process a
  Claude Code session spawns carries `CLAUDECODE=1`, jazyk included when an agent
  drives it, and the adapter's Claude Code inherits it. The refusal reaches jazyk as
  `session/new` answering `Query closed before response received`; the reason is
  on the adapter's stderr (`Claude Code cannot be launched inside another Claude
  Code session`), which the session failure now quotes. The profile clears the
  variable, which is what Claude Code itself says to do:

  ```toml
  [acp.agents.claude]
  command = "npx"
  args = ["--yes", "@zed-industries/claude-code-acp"]

  [acp.agents.claude.env]
  CLAUDECODE = ""
  ```

  The settings reader takes `env` as a subtable, as above; an inline table on the
  `env` key is not read.

- Its `initialize` reply advertises `mcpCapabilities: {http: true, sse: true}`, so
  worker sessions on this profile get the serving over
  [MCP over HTTP](./mcp.md#mcp-over-http). With the variable cleared the adapter
  creates sessions over either transport; `JAZYK_ACP_MCP=stdio` pins the other.
- It asks the client for permission before every tool call, jazyk's tools included,
  unless its permission mode bypasses that. Worker sessions answer by rule
  ([permissions](#permissions)), so no Claude Code flag or setting is needed for
  jazyk tools to run. The adapter reads the user's Claude Code settings (`user`,
  `project`, `local` sources), so a rule there applies too.
- It needs a logged-in Claude Code on the machine (the adapter's auth method is the
  terminal login). `CLAUDE_CODE_EXECUTABLE` in the profile's `env` points it at a
  specific `claude` binary.

## Executors

The `[acp]` profile is the executor for every session unless an override names
another. The [`[executors]` settings](../compiler/project-settings.md#executors) map
a goal kind or a goal class to a profile, so extraction can run on a cheap agent
while GC judgment runs on the strongest one available. E.g.:

```toml
[acp]
agent = "embedded"

[executors]
gc = "claude-code"               # every GC goal kind
reconcile-section = "embedded"   # one compile goal kind
```

Resolution per goal kind, first match wins: the `--agent` flag, `JAZYK_ACP_AGENT`,
`[executors].<kind>`, `[executors].<class>` (`compile` or `gc`), `[acp] agent`, the
built-in default (`embedded`). The flag and the variable name one agent for the whole
run and outrank the table, so a one-off run on another agent needs no settings edit.
The [control plane](../compiler/control-plane.md#executors) owns the resolution. The
rules it obeys:

- A [goal batch](../compiler/reconciler.md#batching) resolves to one executor. The
  scheduler resolves the executor per kind before it batches and never groups goals
  whose kinds resolve to different profiles into one batch, so a worker session is
  created against exactly one agent.
- A GC goal whose cone settles joins the running session only when its executor is
  the session's agent. Otherwise it waits for its own session, which the scheduler
  creates as the next burst.
- Chat sessions, answer sessions, and follow sessions use the `[acp]` agent. The
  overrides apply to goal work only.
- Per-kind and per-class token costs land in `status.yaml` (`costs.by_kind`,
  `costs.by_class`), so the choice is informed by what each kind spends. The resolved
  executor is recorded on the session's trace (`batchStart`, `sessionStart`) and on
  the worker record the control plane writes for the session.

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
  usage. A prose reply in a prompt that already made tool calls gets one nudge before
  the prompt ends: weak models forget they are mid-goal more often than they finish
  silently, and a purely conversational answer still ends the prompt immediately.
  A reply that is empty of both message and calls while carrying reasoning is a
  stall, not an answer: reasoning models narrate the action they intend and stop, as
  if the thinking were visible. The loop answers with a corrective nudge naming that
  ("reasoning is not shown and does not count as acting"), at most twice per prompt,
  before the empty reply is allowed to end it. The stalled reply goes into the history
  with its reasoning as its message text, because an OpenAI-compatible endpoint drops
  reasoning fields from input messages: without that, the model re-thinks from
  nothing each round and stalls again; with it, it reads the plan it just made and
  acts on it. A stall whose finish reason is `length` is a different case: the
  completion cap cut the reasoning before any call, so re-thinking cannot help; the
  nudge says so and asks for the call alone, with no further deliberation, and the
  same two-strike fuse applies. A turn the loop cannot finish (the
  endpoint erroring past its retries) answers as a `refusal` stop with the error
  emitted as a message chunk, never as a protocol error: the client side treats an
  error response to `session/prompt` as fatal to the whole connection, and one
  rate-limited call must not take down the host and every later session with it.
- On `session/new` it also offers the endpoint's models as a session config option,
  so the person in the IDE picks one (below).
- `session/cancel` stops the loop at the next round.
- `session/close` (advertised in its capabilities) tears the session down: each MCP
  server's input closes and the agent waits for it to exit before answering. An
  ephemeral jazyk serving runs its implicit finish on that end of input, so closing
  a worker session is what lands a batch whose agent forgot the finishing call.

The codecs live here: `native` (OpenAI-style `tools` and `tool_calls`, with the
calls for one step batched into a single reply) and `text` (tools described in the
system prompt, one JSON action object per reply, because small models cannot
reliably emit several). The one-per-reply line is the ask, not the parse: a reply
that packs several action objects executes them all in order, one result each,
because executing the first and dropping the rest leaves the model believing work
happened that never did. A text reply that reads as a JSON action but does not parse
is answered with a repair message naming the error, three strikes before the prompt
fails: a dropped brace is a resend, not an answer. The agent probes on the first
round: an endpoint that rejects the `tools` parameter, or a model that answers prose
without calls, downgrades the run to `text`, sticky until it ends. The endpoint
fallbacks ride along too: streaming when the endpoint demands it, dropped
`temperature`, stripped reasoning fields ([LLM settings](../compiler/project-settings.md#llm)).
A reasoning model's reasoning is appended back into the history unchanged, so later
rounds see the reasoning behind earlier calls. No jazyk prompting, no jazyk tool
knowledge, no shortcut into the store. The same session against OpenCode or the
embedded agent carries the same prompt and the same injected tools, which is what
makes the embedded agent a faithful test double for the whole path.

### Choosing a model

The protocol carries the choice: an agent may return `configOptions` from
`session/new`, and the client sets one with `session/set_config_option`. A select
option in the `model` category is what an IDE renders as a model picker.

The embedded agent offers one such option, `model`:

- The values come from the endpoint itself, asked once per session over the
  OpenAI-compatible `/models` listing. An endpoint that does not answer is not an
  error: the configured model is then the only value, which is the state of the world
  before asking.
- The current value is the model the [LLM settings](../compiler/project-settings.md#llm)
  resolved, so an unchanged session prompts exactly what the CLI would.
- A choice applies to the next prompt in that session and to no other, and the answer
  restates the whole option set, as the protocol requires. The option id stays
  `model` across sessions because clients persist a user's default under that id and
  replay it on the next session.
- Nothing else moves: the endpoint and the key stay machine configuration, and a
  session choice never edits `jazyk.toml`. To change what every session starts with,
  set the model in the project or global settings, answer the question
  [`jazyk init`](./cli.md#jazyk-init) asks, or run [`/model`](#slash-commands),
  which does both: it pins the choice in `jazyk.toml` and applies it to the open
  session through the same config option.

For an external agent the options are its own. The proxy forwards
`session/set_config_option` and `session/set_mode` downstream and passes the
`configOptions` it returns back up untouched, so an IDE's model picker drives the
agent behind jazyk exactly as it would without jazyk in the middle.

## Sessions

Three session kinds, all the same protocol:

- Worker sessions: created by jazyk for one [goal batch](../compiler/reconciler.md#batching).
  One [session](../compiler/sessions.md) per batch, so the session keeps a fresh,
  focused context and a retry starts clean. Worker sessions run one at a time:
  [compilation is sequential](../compiler/compilation.md#a-build), and a GC burst is
  the next session, not a parallel one.
- Chat sessions: created by a user in the [GUI chat pane](./gui.md#chat) or from an
  IDE through the proxy. The agent gets the `chat` toolset: the read tools, the
  compilation, binding, generation, and verification lifecycles, the
  [dual-write tools](#dual-write-tools), `update_diagnostic`, `answer_diagnostic`,
  and the [project tools](#project-tools). No raw write tools.
- Follow sessions: read-only mirrors of worker sessions, so a person can watch
  automated work as it happens. In the GUI they appear in the chat pane beside chat
  sessions. Toward IDEs they are served through [session list mirroring](#mirroring-into-ides).

### Session store

A conversation about a project is part of that project's working state, so it is kept
with it: one JSON-lines file per chat session under `<out>/sessions/`, a metadata line
first (id, cwd, agent, start) and then one record per thing said. Both frontends write
it, so a conversation started in an IDE and one started in the GUI pane are the same
kind of object in the same place.

Most agents keep sessions globally under their own home directory, keyed by the
working directory (`~/.claude/projects/<munged cwd>/`, `~/.codex/sessions/`, a SQLite
table with a directory column). Jazyk keeps them per project instead, because every
other thing jazyk remembers already lives in the out directory: the graph, the
journal, the traces. A conversation is about those documents. Deleting the project's
out directory forgets the conversations with it, which is the honest behavior.

What the store serves:

- `session/list` answers with the recorded conversations, newest first, each with the
  first line of its opening prompt as `title` and its last activity as `updatedAt`.
  A client renders that as a history row; without the timestamp the row cannot be
  placed in time. The [mirrored runs](#mirroring-into-ides) follow the conversations
  in the same list.
- A conversation is named the moment it has a first prompt: jazyk pushes
  `session_info_update` with the title rather than making the client wait for the
  next listing.
- `session/load` replays the conversation as `user_message_chunk` and the agent's own
  recorded updates, then answers, because the response is the protocol's
  end-of-replay signal.
- An agent that keeps its own sessions (Claude, Codex, OpenCode all advertise
  `loadSession`) owns the replay instead: its history is the real one, and jazyk
  forwards the load rather than showing the conversation twice. The forwarded
  request gains the jazyk serving the same way `session/new` does, so a reopened
  session keeps its tools.
- A load never reaches an agent that did not advertise `loadSession`. The proxy
  answers it: it opens a fresh downstream session for the continuation, routes the
  loaded id onto it (prompts, cancels, and configuration one way, updates and
  permission requests the other), replays whatever the store holds, and responds.
  A conversation with no recorded history still loads, with a note saying so; a
  missing transcript is not a launch failure.
- After any load, the proxy re-advertises the [slash commands](#slash-commands) and
  the store keeps appending to the same conversation file, so a reopened session
  behaves like the one it continues.

## Worker sessions

The automated path. For each goal batch the runner:

1. Resolves the batch's [executor](#executors) and creates a session on that agent
   whose `mcpServers` list one entry: `jazyk mcp` with the batch's toolsets and the
   spawning flags (`--only <batch>`, `--packaged`, `--ephemeral`, `--build-token`;
   see [MCP into sessions](./mcp.md#mcp-into-acp-sessions)). The batch id
   (`b<generation>-<n>`) names the session, its lease, and its trace label. The
   toolset is the union of what the batch's goal kinds need
   ([toolsets](../compiler/sessions.md#toolsets)).
   The entry's transport follows the agent's `initialize` reply, decided once per
   agent when its host starts:
   - Stdio is the default: the entry names the command and the agent spawns it.
   - HTTP when the reply advertises `mcpCapabilities.http`: jazyk starts one
     [MCP over HTTP](./mcp.md#mcp-over-http) server for the session (loopback, a
     random port, a per-session bearer token in the entry's headers) and lists it as
     an `http` server. The server stops when the session closes.
   - Protocol version 1 has no stdio flag (the transport is mandatory on paper), so
     an agent that advertises HTTP is taken at its word. `JAZYK_ACP_MCP` pins the
     choice for a run (`stdio` or `http`;
     [environment tuning](../compiler/project-settings.md#environment-tuning)).
2. Prompts with the batch's contract itself: the
   [assembled session prompt](../compiler/sessions.md#the-prompt) (the agent
   contract, the active skills, the project block, the goals block, the
   [loaded set](../compiler/context.md#the-loaded-set), and the worker protocol line
   naming the batch) travels as the session prompt (a prompt is what a model reads
   best), and the serving's `begin_goals` answers with a short ack (`--packaged`).
   Binding and generation packages ride the `begin_*` reply instead. The prompt
   source is the same assembly either way; only the channel differs.
   [`jazyk preview`](../compiler/sessions.md#preview) prints exactly the text this
   step sends.
3. Reminds once when the prompt ends in prose: an agent that answers without tool
   calls ends its prompt by design, so when the prompt ends, the batch has not
   committed, and the watchdog did not fire, the runner sends one follow-up prompt in
   the same session ("the goals are not resolved; continue with tool calls, finish
   with `done`"). The agent stays generic; the client owns the reminder. One reminder
   per batch; a second prose ending is a failed session.
4. Consumes the update stream and translates it into
   [trace events](../compiler/sessions.md#trace-events): message and thought chunks
   become model text, `tool_call` and `tool_call_update` become tool rows, usage
   updates accumulate into the token count, and `mark_goal_done` and
   `mark_goal_failed` calls become `goal` events carrying the justification or the
   reason. The trace, the transcript, and the GUI panels do not care which agent ran
   the session.
5. Closes the session and waits for the teardown, so an agent that ended its prompt
   with staged work but no finishing call still lands it: the serving's implicit
   finish runs on the teardown, under the same gates the
   [budget path](../compiler/sessions.md#budgets) uses.
6. Reads success from the store, never from the agent's word: the commit happened
   inside the MCP serving under its own gates, so the runner attributes the journal
   entries between the session's start and end generations to the batch, and every
   goal in the batch must have been resolved (`mark_goal_done` accepted at commit)
   or failed with a reason. A goal the session neither resolved nor failed parks
   ([resolving, failing, parking](../compiler/sessions.md#resolving-failing-parking)).
   A session whose batch did not land is a failed session, whatever the agent said.
   A retry is a fresh session; its claim on the same batch is re-entrant, so its own
   earlier attempt never blocks it.

A session that goes silent is cancelled after an idle timeout (`JAZYK_ACP_IDLE_TIMEOUT`,
default 600 seconds). Cancellation follows the protocol: pending permission requests
are answered `cancelled`, then `session/cancel` goes out. The lease TTLs bound what a
dead agent can hold either way.

A host process that dies would otherwise take every later session with it: session
creation and prompts fail with `acp host is gone`. The runner treats that as the
host's death, not the batch's: the cached host is dropped, the next session (a batch
retry included) spawns a fresh one, and only a spawn that fails again fails its batch.
An agent that answers `session/new` with an error takes the host driver with it (the
client library treats the error as fatal to the connection), so the session's failure
carries the agent's error and the last lines of the agent's stderr:
`session: acp host for claude ended while creating a session: <the error>; agent
stderr: <its last lines>`. The host keeps those lines for exactly this: an agent's
refusal is usually explained only there.

## Chat sessions

A chat session is an open conversation with the agent about the project. The injected
`chat` serving carries the read tools, the compilation, binding, generation, and
verification lifecycles, the [dual-write tools](#dual-write-tools),
`update_diagnostic`, `answer_diagnostic`, and the [project tools](#project-tools),
and no raw write tools, so "tighten this requirement and recompile" is a sentence,
not a workflow.

### Dual-write tools

A fact with quote provenance lives in the prose; the graph carries its compiled form
and the verbatim quote. A chat edit must move both or neither. The four tools are the
chat form of the [edit paths](../compiler/compilation.md#edit-paths):

- `revise_requirement({id, new_text, statement?})`: locates the requirement's quote
  in the document, stages the prose replacement and the requirement update
  (`statement` when given, the new sentence as the quote) as one changeset, and
  commits them atomically. The document's stored content hash updates in the same
  commit, so the edit does not dirty the document it just reconciled; downstream
  goals (`rejudge-pair`, `bind`) derive from the graph change instead.
- `add_requirement({doc, section, after_quote?, text, statement, entities})`:
  inserts `text` into the section (after the located `after_quote`, or at the
  section's end) and creates the requirement with `text` as its quote, one changeset.
  The entities must exist; search before naming them.
- `retract_requirement({id, reason})`: removes the sentence from the prose and
  deletes the requirement, one changeset. The deletion writes its
  [change records](../compiler/graph.md#change-records), so a view or instance that
  referenced the requirement gets a `retrace` goal on the next build.
- `edit_fact({id, field, value, note?})`: any authored field on any node (an
  entity's `definition` or `parent`, an attribute value, a requirement's `edges`, a
  view's members). When the fact is quote-provenanced, the agent proposes the
  sentence rewrite in conversation, the person accepts it, and the call carries the
  accepted sentence as `note`: the serving locates the quote and commits the prose
  replacement with the graph mutation as one dual write. Without an accepted
  sentence, or when the fact is `derived` or `decree`, the edit lands graph-only
  with `decree` provenance (`note` becomes the decree's note) and a
  [ratification proposal](../compiler/model/diagnostic.md#ratification-proposals)
  follows. The compiler never rewrites a source document without an accepted
  sentence. An `edit_fact` that names a default view makes it curated: the view
  stops being default and survives recomputes
  ([default views](../compiler/model/view.md#default-views)).
- The prose edit surfaces to the ACP client as a file write plus a diff on the tool
  call, so an IDE shows it in the buffer and the review UI. See
  [doc edit delegation](#doc-edit-delegation).
- Every dual write journals a `dual-write` entry, every decree a `decree` entry
  ([journal](../compiler/graph.md#journal)), so `jazyk ripple` roots the cascade at
  the chat edit.
- Direct graph writes without a prose edit are not in the `chat` toolset. That path
  remains only in `jazyk mcp graph --write`, the debugging surface.

### Project tools

Setup and configuration are chat tools too, routed through the same edit delegation:

- `init_project`: scaffold `jazyk.toml` and the starter layout, what
  [`jazyk init`](./cli.md#jazyk-init) writes. Offered only where there is nothing to
  scaffold onto: a serving whose directory already holds a `jazyk.toml` does not list
  the tool at all, because the answer could only ever be a refusal. The serving's
  instructions state which case it is in, so the agent knows whether it is talking
  about a project or about an empty directory without calling anything.
- `update_project_settings`: typed edits to `jazyk.toml` (workflow modes, docs glob,
  lint rules, the `[acp]` profile, the `[executors]` overrides), rendered as minimal
  edits, never a whole-file rewrite. An uninitialized directory has no settings to
  edit, so this tool is offered only in a project.

### Questions in chat

Open diagnostics that carry a [prompt](../compiler/model/diagnostic.md#prompts) are
the project's standing questions, and a chat session is where they get asked. Each
one is a blocked goal on the board (an [`answer`](../compiler/goals/answer.md) goal,
or a [`ratify`](../compiler/goals/ratify.md) goal for a ratification proposal), so
the build's verdict counts them until they are answered:

- On session start, when open prompted diagnostics exist, jazyk sends one summary
  message into the session (count and the top questions with their options), so
  opening a project with existing errors and warnings re-surfaces them without any
  request. `/questions` lists them again at any time.
- A person answers in plain chat ("apply the first option on diag:contradiction-2",
  or a freeform reply). The session's agent records it with `answer_diagnostic`:
  - an `edit` option applies deterministically inside the serving (dual write,
    diagnostic resolved) before the tool returns; no model judgment touches it. A
    ratification proposal's `edit` option is this path: the proposed sentence lands
    in the document and the fact's provenance flips to `quote` in the same
    changeset, which journals a `ratify` entry and resolves the `ratify` goal.
  - an `answer` option or freeform text is recorded on the node, and the tool's
    reply hands the handling contract to the same agent: act on the reply with the
    session's tools, then `resolve_diagnostic` (or `update_diagnostic` to refine the
    question and leave it open). The commit journals an `answer` entry.
- The agent can also author and edit questions: `report_diagnostic` accepts a
  `prompt` (the `decision` rule is the shape for a design choice the documents leave
  open), and `update_diagnostic` sets a new one on an existing finding. A question the
  agent sharpens in chat is the same question the LSP shows inline in the file.

### Answer sessions

Answers arriving outside a chat session (an LSP code action, the GUI panel) still
need a model when they are not suggested edits. Jazyk spawns one focused session for
the answer, the same shape as a worker session: the `chat` serving injected, the
prompt carrying the diagnostic, the loaded set for its subjects, the question, and
the human's reply, with the contract to act on the reply and then resolve or
re-prompt the diagnostic. The `answer.status` on the node moves `handling` on spawn
and `handled` or `failed` when the session lands, so every frontend shows the same
progress from the store. The `answer` goal resolves when the session lands.

### Slash commands

Chat sessions advertise commands through `available_commands_update`. ACP has no
invoke method for commands; they arrive as prompt text. Jazyk matches the prefix
before the prompt reaches the agent: a matched command runs the real work (the same
path as the CLI command of that name) and streams its progress into the open prompt,
then ends it. Unmatched prompts go to the agent as conversation.

A build command streams the work at full fidelity, not just its boundaries:

- Batch and session boundaries, and jazyk's own narration (the board summary line,
  `gc burst:` lines, the verdict with its counts), are message text. Narration is
  not thinking, so it never renders as a thought.
- The worker's reasoning (`modelText` trace events) flows as `agent_thought_chunk`,
  so the minutes inside a session are visible thinking, not silence.
- Each graph tool call flows as `tool_call` titled by the decision, not the tool
  name: `add entity store`, `coverage /tiny covered`. When the result settles what
  happened, the completed `tool_call_update` retitles the row (`added entity
  ent:store` against `updated entity ent:store`) and carries the output; a failed
  call carries the violated rule. The raw arguments ride as the row's input. Ids are
  namespaced per session (`jazyk:<label>:<n>`), so a chat session's own calls never
  collide with the build's in the client's tool-call table.
- A `mark_goal_done` call flows as a row titled by the goal (`resolved
  g:reconcile-section:docs/orders.md#/orders/holds`) with the justification as its
  output; `mark_goal_failed` the same, titled `failed`, with the reason.
- The lifecycle calls (`begin_goals`, `done`) get no row: the person asked for the
  build, so its machinery is not news. What the `done` call says lands where it
  belongs: the closing line carries the model's own summary of what it did.

A command exists when a person needs an answer jazyk can give exactly, and no model
should be improvising it: what this project is set to, what state the build is in,
what setup remains. The catalog:

| Command | What it does |
| --- | --- |
| `/help` | What jazyk is, and every command in this session with one line each. |
| `/init` | Set the project up: scaffold what is missing, then state what is still unanswered (agent, model) and the command that answers it. |
| `/config` | The project's settings and where each came from. With arguments (`/config llm.model qwen3`), a minimal edit to `jazyk.toml`. |
| `/model` | The models the endpoint serves, the current one marked. With a name, pins it in `jazyk.toml` and applies it to this session where the agent takes the `model` config option. |
| `/agent` | The agents jazyk can drive (built-in and configured), the current one marked. With a name, records it in `jazyk.toml`; the switch takes effect when the IDE restarts the jazyk agent. |
| `/status` | The last build: verdict with its counts, graph size, open findings, board counts. |
| `/board` | The goal board as `jazyk compile` would derive it now: open goals by class and kind, the batches the scheduler would form, blocked goals with their reasons, parked and failed goals. The verdict when the board is empty. |
| `/preview` | The next session's prompt, exactly as the model would receive it. With a goal or target (`/preview ent:order`), the batch that goal would join. What [`jazyk preview`](../compiler/sessions.md#preview) prints. |
| `/explain` | Why a goal exists, or what a change to a target would open. With a goal, its change record, cause, readiness, and hints; with a target, the cone of goals a change there would open. What [`jazyk explain`](./cli.md#jazyk-explain) prints. |
| `/ripple` | Walk a change's cascade through the journal: the generations it led to, the goals each opened, the sessions that resolved them. `--back` walks upstream to the edit that started it. What [`jazyk ripple`](./cli.md#jazyk-ripple) prints. |
| `/questions` | The [standing questions](#questions-in-chat) on open findings. |
| `/compile` | Reconcile the graph with the documents: run the board to convergence. |
| `/generate` | Bind and generate the deliverable. |
| `/verify` | Run verification over the ledger. |
| `/release` | Approve pending work in manual mode. |

The catalog is one list, served identically by the IDE proxy and the
[GUI pane](./gui.md): a command means the same thing in both, and neither invents its
own. What differs is only how the work is started, because the GUI already has a job
queue and the proxy does not.

The list follows the directory. A session outside a project advertises `/help` and
`/init` and nothing else: no build command has anything to build yet, and offering
the rest when every one answers "not a jazyk project" reads as breakage.

Jazyk's names win over the downstream agent's. An agent that advertises its own
`/init` is shadowed inside a jazyk project, which is the intended trade: in a jazyk
session, `/init` is the project's own setup.

## Plans

Build progress is an ACP plan. The runner publishes one plan entry per goal batch,
keyed by the batch id and titled by the batch's locality and goal kinds ("reconcile
docs/cli.md (3 sections)", "rejudge req:order-3 ~ req:cart-2", "abstract ent:order",
"generate ent:order", "verify req:order-3"), and flips each entry `pending` →
`in_progress` → `completed` as the build advances. The pending entries are the
batches the scheduler projects from the current board; every commit re-derives the
board, and the plan is republished whole with the projection re-formed, per the
protocol's replace semantics. A blocked goal appears as its own pending entry
carrying the reason ("blocked: awaiting answer on diag:contradiction-2"), so a plan
that ends with blocked entries is the same statement the verdict makes. The GUI
renders the plan as a live checklist; an IDE that triggered the build through
`/compile` sees the same plan inside its prompt.

## Permissions

Two policies, chosen per session kind:

- Worker sessions answer permission requests by rule: reads and jazyk tool calls are
  allowed, anything touching files outside the project or the deliverable is
  rejected. Automated work never blocks on a human.
- Chat sessions forward permission requests to the user: the GUI shows them in the
  pane, the proxy passes them through to the IDE. An unanswered request cancels with
  the prompt.

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
  worker sessions (each on its resolved [executor](#executors)) and streams
  synthetic tool calls and the [plan](#plans) into the open IDE prompt.
- Outside a jazyk project (no `jazyk.toml` above the session's `cwd`), the proxy is a
  transparent passthrough plus `/help` and `/init`. `/init` scaffolds a project and
  the proxy adopts it immediately: the commands it runs itself work in the open
  session, and because a session's tools are injected when it is created, the agent's
  own jazyk tools arrive with the next session. The reply says so. The IDE's global
  agent entry works everywhere; jazyk lights up only where a project exists.

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
  read-only sessions titled by their batch ("reconcile docs/cli.md"). An IDE with a
  session picker attaches through the standard `session/load`, which replays the full
  history (tool calls included) and then streams the live tail. A prompt typed into a
  mirrored run is answered by the proxy with a note that the session is a read-only
  mirror; it is never forwarded to the agent, which has no such session.
- `_jazyk/session_list_changed` is a custom notification (underscore-namespaced, so
  clients that do not know it ignore it, per the protocol's extensibility rules)
  telling a capable client to refresh its session list. The GUI honors it; plain IDEs
  lose nothing.
- Background builds additionally push the [plan](#plans) and a session info update
  into the most recently active jazyk session, and only that one. Rendering of
  updates outside a prompt varies by IDE; the GUI always renders them.

### LSP and the proxy

The [LSP](./lsp.md) stays read-only and never compiles. What it knows (stale
documents, open diagnostics, open goals) reaches the IDE's chat surface through the
proxy instead: on session start and on every
[control plane](../compiler/control-plane.md) or board change, the proxy refreshes
the advertised commands and the pending-work plan. The board derives from files (the
documents, the graph, `status.yaml`) and the leases are files, so the proxy computes
them the way every other frontend does, no new channel.

## Registration

Every ACP client spawns its agent as a child process over stdio, so a registration is
always the same three facts: a name, a command, and its arguments.
`jazyk acp install --ide <client>` writes them where that client keeps them, and
[`jazyk init`](./cli.md#jazyk-init) offers the same step.

Clients register agents globally, not per project (Zed and JetBrains both, by
design). Per-project behavior comes from the protocol instead: every session names
its `cwd`, and the proxy resolves the project from there, so one global entry serves
every project and stays inert outside them.

The clients jazyk writes config for:

- `zed`: `~/.config/zed/settings.json`, under `agent_servers`. A custom agent states
  its kind: `"type": "custom"` alongside `command` and `args`.
- `jetbrains`: `~/.jetbrains/acp.json`, under `agent_servers`, with `command` and
  `args` and no kind.
- `vscode`: the user `settings.json`, under `acp.agents`, read by the ACP client
  extension (VS Code hosts no agent of its own).

The clients jazyk prints a snippet for, because their config is a program (Neovim's
Lua, Emacs' Lisp) or another application's private state (the Obsidian plugin's vault
data), and rewriting either is not jazyk's business: `neovim`, `emacs`, `obsidian`,
`acpx`, `marimo`.

Two rules hold for every written config:

- The command is the name the user runs, `jazyk`, whenever that name resolves to this
  same binary on `PATH`; the absolute path only when it does not. A registration
  written as a path into a build directory breaks on the next rebuild or move.
- These files are kept by hand. The merge is a splice at the entry's own position, so
  comments, trailing commas, formatting, and every other agent survive it. A rerun
  that would change nothing writes nothing; a stale `Jazyk` entry is corrected in
  place rather than duplicated.

## Protocol versions

The internals are modeled on protocol version 2 even while the wire speaks version 1:

- Tools reach agents only through injected MCP servers. Jazyk never relies on the
  protocol's file system or terminal methods toward agents; version 2 removes them.
- Progress is consumed from the update stream, not from the pending `session/prompt`
  response.
- Tool calls are treated as upserts by id, and unknown enum values are tolerated.

The wire version is negotiated per connection in `initialize`, in both roles. Today's
agents and IDEs answer version 1; version 2 engages per peer as peers ship it. The
one version 1 convenience in use is the upstream file write in
[doc edit delegation](#doc-edit-delegation), which degrades to a direct disk write.

## Configuration

The `[acp]` section of `jazyk.toml`, the global `~/.jazyk/config.toml`, the
`JAZYK_ACP_AGENT` environment variable, and the `--agent` flag resolve per field like
the [LLM settings](../compiler/project-settings.md#llm) do. The `[executors]`
section overrides the profile per goal kind or class ([executors](#executors)). See
[project settings](../compiler/project-settings.md#acp).
