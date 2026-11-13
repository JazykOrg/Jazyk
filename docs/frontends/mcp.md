# MCP

A project opens all [document sets](../compiler/document-sets.md). Graph operations address one set
explicitly. Each set owns `.jazyk/<set-id>/`; project file operations share the
physical files and report every membership. The document-set contract defines
configuration, aggregate reads, management commands, and grouped compilation.

The project tool `read_document` takes a project-root-relative `path`.
Resolve links inside a document from that document's directory before calling
it, as described in [file paths](./agent-workflows.md#file-paths). It reads Markdown or a
[native DOCX/PDF projection](../compiler/binary-documents.md#diagnostics-and-agents)
with source locations and diagnostics. It does not return binary base64 to agents.
`line` (1-based) and `limit` (a line count) select part of a text document. A reply
carries at most 64 KiB of `text`; a cut reply carries `truncated: true`,
`totalLines`, and `nextLine`. A single line longer than the cap returns a UTF-8-safe
prefix with `truncated: true` and `truncatedLine`. `hash` always covers the whole
document.

`jazyk mcp <toolsets>` serves the [tool registry](../compiler/tools.md) over stdio as an
MCP server: line-delimited JSON-RPC per the Model Context Protocol. It dispatches the
same tool implementations the compiler's own [sessions](../compiler/sessions.md) use.
There is one registry, not a second API beside it.

The serving has two audiences: an agent that connects on its own (the standalone
loops below), and an [ACP session](./acp.md) that gets the serving injected per
session ([MCP into ACP sessions](#mcp-into-acp-sessions)). Same tools, same gates.

## Toolsets

The argument names what the serving is for. One serving can carry several, comma
separated; each adds its tools to the union:

- `jazyk mcp compile`: the [compilation lifecycle](../compiler/tools.md#compilation-tools),
  the [write tools](../compiler/tools.md#write-tools), the
  [view tools](../compiler/tools.md#view-tools), and the
  [goal tools](../compiler/tools.md#goal-tools). The agent claims goal batches
  from [the board](../compiler/compiler.md#components) and resolves them.
- `jazyk mcp generate`: the [binding tools](../compiler/tools.md#binding-tools) and
  the [generation tools](../compiler/tools.md#generation-tools). The agent searches
  and writes the deliverable with its own editor and shell; jazyk holds the ledger.
- `jazyk mcp verify`: the [verification tools](../compiler/tools.md#verification-tools).
  Small enough to hand a subagent whose only job is judging one row.
- `jazyk mcp runtime`: the nine [task runtime tools](../compiler/task-runtime.md).
  This serving needs project settings but no compiled graph. It carries the execution
  instructions and no document-set or graph tools. Combine it with another toolset
  when the agent also needs that workflow.
- `jazyk mcp decompile`: the
  [decompilation tools](../compiler/tools.md#decompilation-tools). The agent reads
  the code with its own tools and submits prose drafts; jazyk validates and lands
  them in the docs tree. Decompilation stays outside the board.
- `jazyk mcp benchmark`: the agent under test performs the
  [benchmark cases](../benchmark/benchmark.md#agent-run-benchmarks) against sandbox
  stores and is graded by the same deterministic checks as an endpoint run. Built to
  hand a subagent: the serving is self-contained and touches no project state beyond
  the machine-wide history its report appends.
- `jazyk mcp graph`: the read tools only, for retrieval consumers. `--write` adds the
  raw write tools, the view tools, and the task runtime tools without the lifecycle, for debugging and manual
  graph surgery; each write commits as its own changeset, and the commit writes its
  [change records](../compiler/graph.md#change-records) so the next build derives
  the goals the write opened.
- `jazyk mcp all`: `compile`, `generate`, `verify`, `decompile`, and `chat` in one
  serving, the union an agent working a whole project needs, so a config names one
  entry instead of listing them. `benchmark` stays out (it performs cases against
  sandbox stores, not the project) and so do the raw write tools of `graph --write`
  (a write outside a batch is graph surgery, served on purpose). `jazyk init` writes
  this one ([init](./cli.md#jazyk-init)).
- `jazyk mcp chat`: the serving for [chat sessions](./acp.md#chat-sessions): read
  tools, the [compilation](../compiler/tools.md#compilation-tools),
  [binding](../compiler/tools.md#binding-tools),
  [generation](../compiler/tools.md#generation-tools), and
  [verification](../compiler/tools.md#verification-tools) lifecycles, the
  [dual-write tools](./acp.md#dual-write-tools), the decree tools `retract_decree`
  and `bump_limit` ([chat tools](../compiler/tools.md#chat-tools)), `update_diagnostic` and
  `answer_diagnostic` ([questions in chat](./acp.md#questions-in-chat)), and the
  [project tools](./acp.md#project-tools). Compiler write and goal tools are available
  for a claimed batch. Writes stage under that batch's toolset and gates. Outside a
  batch, authored graph mutations commit directly with the same integrity checks.
  `edit_graph` groups related edits into an independent atomic transaction without
  claiming goals or releasing compilation. It remains available for a separate
  user request while a batch is open; a committed change makes that batch stale.
  Scoped workers keep their goal restrictions. A conversational edit uses the dual-write tools, moving
  prose and graph together. In a browser tab the serving leaves
  out the binding, generation, and verification lifecycles, which need a shell
  ([chat in a tab](./wasm.md#chat)).

Every serving includes the [read tools](../compiler/tools.md#read-tools) (`load`,
`expand`, `unload`, `graph_status`, `search`, `read_section`, `get_entity`,
`get_view`, `diagnostics`), [`report_feedback`](../compiler/tools.md#feedback-tool),
[`report_spend`](../compiler/tools.md#spend-tool) (what an external agent's run
billed, recorded on the open batch's journal entry), `await_changes`, and
[`recent_edits`](#watching-edits) (below). The reads maintain the
[loaded set](../compiler/context.md#the-loaded-set) per open batch, and every
mutating reply carries its condensed status block. A reply's text content is the JSON
object pretty-printed ([tools](../compiler/tools.md)), so the agent's user reads it in
the client's tool call view the way the model does.

A refusal is a tool result with `isError: true` whose text is `{error: {rule,
message}}`, so the agent reads and repairs it like any reply. A gate, a missing or
invalid argument, a read-only serving, a busy project, and a project file that does
not load all refuse this way. A JSON-RPC error answers only what is not a call the
serving can judge: an unknown method, a request without a tool name, a tool the
serving does not serve, and a stopped or suspended run.

Set-scoped Bind and Generate workers also receive `list_document_sets`,
`preview_document_set` and `update_document_set`. They can repair the current
set's `docs`, `source` and `roots` selections without leaving the worker. Updates
use `baseHash` from the list reply and the singular `set.source` field. They keep
the selected set's id. Creating, removing, restoring or directly editing another
set is refused. The normal role resolver still excludes overlapping source and
document selections within this set. A repair that would change another set's
selection is refused toward project settings. A preview does not write.

The same worker uses the updated selections on its next file, ledger or board
call. Recovery checkpoints record that configuration hash. Resuming after an owned
matcher repair accepts the saved configuration and preserves the session's work.
An unrelated configuration change still refuses restoration.
The parent binding and generation gates also read the current selected set's Source
mapping.
The command retains its admitted settings for checkpoint identity. Matcher repairs
do not restart completed groups, including a forced generation. Current settings
still govern execution and file authorization. Existing checkpoints without an
admission record use their current unchanged settings.
A source-pattern repair does not authorize a document write, a file outside
the project, or a file owned by another entity. Read-only, graph, verification and
runtime workers do not gain these matcher writes. An owned build can edit under
its existing project lock only when its unique attempt token matches the current
build lease. A replacement build never inherits the previous attempt's token.
The short configuration lock and `baseHash` comparison still protect replacement.
Other callers retain the normal project-lock and build-lease refusals.

On a compile serving, `done` is the one finishing call: the session prompt says
`done`, so the model never translates a verb.

## MCP into ACP sessions

When jazyk creates an [ACP session](./acp.md#worker-sessions), it injects one MCP
server into it: `jazyk mcp` with the batch's toolsets and flags. The flags exist for
this spawning path and are not for standalone servings:

- `--ephemeral`: the serving belongs to one session. It does not register in the
  [worker registry](../compiler/control-plane.md#workers-and-leases) (the session is not
  a peer worker, it is part of a run that already holds its lease), and end of input
  with an open batch runs the implicit finish: staged work commits under the same
  gates the [budget path](../compiler/sessions.md#budgets) uses, so an agent that dies
  mid-batch still lands its valid extractions. It does not serve `await_changes`
  or `recent_edits`:
  the session exists for one batch, and a long poll is a stall wearing a tool's name
  (a weak model was observed idling a build on it instead of calling `done`).
- `--only <batch|goal>`: `begin_goals` accepts only the named batch
  (`b<generation>-<n>`), or the batch the named goal (`g:...`) belongs to. A
  session's serving cannot claim work beyond its batch, so a session that finishes
  early never wanders onto the rest of the board. Once its batch commits, the serving
  answers every write, goal tool, `done`, and `abandon_goals` with `session-complete`,
  naming the batch and its generation: the session's work is done and the agent ends
  its turn. A plain `no-open-batch` there would send the agent back to `begin_goals`
  (a local model once re-marked goals of a committed batch eight times).
- `--build-token <id>`: the serving is part of the running internal build. The
  build-lease refusal and the release gate do not apply to its batch; without this,
  a build's own sessions would deadlock against the build's own lease.
- `--packaged`: the bridge already delivered the
  [assembled prompt](../compiler/sessions.md#the-prompt) as the session prompt;
  `begin_goals` answers with a short ack instead of repeating it.
- `--serve-files`: adds the file and command tools
  ([generation tools](../compiler/goals/generate.md#tools)) for agents whose
  profile sets [`serve_files`](../compiler/project-settings.md#acp). Verify workers
  receive only file reads and listing. Agents with their own editor never get them.
- `--all-tools`: list every served tool from the start instead of a
  [skill-gated catalog](./agent-workflows.md#skill-gated-tools), for a client that
  does not refresh its tool list on `notifications/tools/list_changed`.
- `--edit-sink <path>`: delegate document and settings writes to the spawning
  process ([doc edit delegation](./acp.md#doc-edit-delegation)). Absent, or with
  nothing listening, writes land on disk directly.

The `initialize` reply answers the client's requested `protocolVersion` when the
serving supports it (`2024-11-05`, `2025-03-26`, `2025-06-18`, `2025-11-25`), and its
latest supported version otherwise, so a client never reads support for a version the
serving does not speak. A request without a version gets `2024-11-05`.

A `jazyk.toml` that does not load leaves `initialize`, `ping`, and `tools/list`
answering, so the agent can still connect and repair the file. The instructions name
the error, and every tool call that needs the project is refused with it.

The `initialize` reply carries server instructions describing the work loop for the
toolsets served, so an agent that reads nothing else still knows the entry point, the
order of calls, and that a tool error names its own repair. They also say that the
[jazyk section of `AGENTS.md`](./agent-context.md#the-jazyk-section-of-agentsmd) is
orientation and that these instructions and tool replies take precedence over it. A serving ends when its
input ends: close stdin to tear it down. `shutdown` is answered with null for agents
that send it out of LSP habit; it does nothing. On stdio a line that is not JSON
answers a parse error (`-32700`) with a null id, a message that is not a request
object answers an invalid request error (`-32600`), and a batch array answers an
array of the responses its requests produced (none for a batch of notifications). The prompting lives in three places,
most specific wins: server instructions carry the loop, each `begin_*` reply carries
the batch's own assembled prompt (or the package, for binding and generation), and
each tool description names what to call next.
Graph-write guidance appears only for a graph-writing toolset. A ledger serving's
instructions do not advertise graph mutations or an unavailable long poll.

The injected serving reaches the agent over one of two transports. Stdio is the
default: the `session/new` entry names the `jazyk mcp` command and the agent spawns
it. An agent that speaks MCP only over HTTP gets the same serving over
[MCP over HTTP](#mcp-over-http) instead; the choice is made per agent from its
`initialize` reply ([worker sessions](./acp.md#worker-sessions)).

## Workflow skills

Every serving except the dedicated task runtime exposes `list_jazyk_skills` and
`read_jazyk_skill`. They return the shared built-in workflow instructions without
opening a batch or changing state. The [main policy](./agent-workflows.md) is the
same template used for `AGENTS.md`, ACP, and Cloud chat. Capabilities and active
packages constrain what the agent can do.

## Agent setup

Every coding agent that reads MCP configuration can run jazyk's tools without the
[IDE proxy](./acp.md#the-ide-proxy): its config gets one server entry named `jazyk`
serving `jazyk mcp all`. `.mcp.json` is the canonical project file: every project
setup writes the `jazyk` entry there too, whichever agents are chosen, because
Claude Code, VS Code, and jazyk's own [agent context](./agent-context.md#mcp-servers)
read it. A row's own file is written only for an agent that does not read
`.mcp.json`. The setup also keeps the [jazyk section of
`AGENTS.md`](./agent-context.md#the-jazyk-section-of-agentsmd) current. The [launcher](./cli.md#ide-agents) and
[`jazyk init`](./cli.md#jazyk-init) write these entries, and
[`jazyk setup`](./cli.md#jazyk-setup) lists and removes them; each agent has a setup
name.
The command in every entry follows the [registration rule](./acp.md#registration):
`jazyk` when that name on `PATH` is this binary, the absolute path otherwise, with a
warning that the entry breaks when the binary moves.

Each agent is set up by the first method that works for it:

- Project file: the agent's project config in the repository. Merged in place,
  keeping comments, formatting, and other entries; a stale `jazyk` entry is corrected
  where it is.
- Global file: the agent's user config, for an agent with no project config. It asks
  before writing and names the file. `jazyk mcp` started outside a project serves the
  same tools, so one global entry works for every project.
- Guided step: for an agent whose MCP servers live in application state jazyk must
  not write. The summary prints the menu path and the JSON to paste; nothing is
  written, so the step alone does not count as a setup.

| name | agent | detected by | setup | file or step |
|---|---|---|---|---|
| `claude` | Claude Code (CLI, VS Code extension, JetBrains plugin) | `claude` on `PATH`, the `anthropic.claude-code` extension, a JetBrains plugin named `claude` | project file | `.mcp.json`, and `jazyk` in `enabledMcpjsonServers` of `.claude/settings.local.json` |
| `codex` | Codex (CLI, VS Code extension) | `codex` on `PATH`, the `openai.chatgpt` extension | project file | `.codex/config.toml` (`[mcp_servers.jazyk]`) |
| `gemini` | Gemini CLI, Gemini Code Assist | `gemini` on `PATH`, the `google.geminicodeassist` extension | project file | `.gemini/settings.json` (`mcpServers`) |
| `opencode` | OpenCode | `opencode` on `PATH` | project file | `opencode.json` (`opencode.jsonc` when that is the file the project has) |
| `pi` | Pi | `pi` on `PATH` | project file | `.pi/mcp.json` |
| `droid` | Factory Droid | `droid` on `PATH` | project file | `.factory/mcp.json` |
| `cursor` | Cursor's agent | Cursor installed | project file | `.cursor/mcp.json` |
| `copilot` | GitHub Copilot in VS Code | VS Code installed (it ships Copilot Chat built in), or the `github.copilot-chat` extension | project file | `.vscode/mcp.json` (`servers`, `"type": "stdio"`) |
| `zed` | Zed's agent | Zed installed | project file | `.zed/settings.json` (`context_servers`) |
| `junie` | Junie | a JetBrains plugin named `junie` | project file | `.junie/mcp/mcp.json` (`mcpServers`) |
| `roo` | Roo Code | the `rooveterinaryinc.roo-cline` extension | project file | `.roo/mcp.json` (`mcpServers`) |
| `ai-assistant` | JetBrains AI Assistant | a JetBrains plugin named `ml-llm` | project file | `.ai/mcp/mcp.json` (`mcpServers`), the project file the plugin watches |
| `copilot-cli` | GitHub Copilot CLI | `copilot` on `PATH` | global file | `~/.copilot/mcp-config.json` (`mcpServers`) |
| `cline` | Cline | the `saoudrizwan.claude-dev` extension | guided step | the MCP Servers icon, Configure, Configure MCP Servers, paste the JSON |

Extensions are looked up in the IDE's extension folder (`~/.vscode/extensions`,
`~/.cursor/extensions`); JetBrains plugins in the IDE's user plugin folder and in its
bundled plugins. An agent detected through its program counts in every IDE; one
detected through an extension or plugin counts in the IDE that has it.

A known coding agent with no setup row (`aider`, `amp`, `goose`, `kiro-cli`, `qwen`,
`auggie`, `crush`) is listed dim as `unsupported` when its program is on `PATH`.

### Reaching each agent

The launcher's summary names where each agent is reached in the IDE it started:

| agent | where |
|---|---|
| the Jazyk agent, VS Code or Cursor | the ACP Client icon in the Activity Bar, Agents view: Jazyk |
| the Jazyk agent, Zed | the Agent panel, the new thread menu: Jazyk |
| the Jazyk agent, JetBrains | the AI Chat window, the agent selector at the bottom: Jazyk |
| `claude` | jazyk tools load once Claude Code trusts the folder |
| `codex` | jazyk tools load once Codex trusts the folder |
| `gemini`, `copilot-cli` | `/mcp` in the agent lists jazyk |
| `opencode`, `pi`, `droid` | jazyk tools load on the agent's next start |
| `cursor` | Cursor Settings, MCP: jazyk, enabled |
| `copilot` | Chat in Agent mode: jazyk in the tools picker |
| `zed` | the Agent panel settings, MCP servers: jazyk |
| `junie` | Junie's MCP servers: jazyk |
| `roo` | Roo's MCP servers: jazyk |
| `ai-assistant` | Settings, Tools, AI Assistant, Model Context Protocol (MCP): jazyk |

## MCP over HTTP

Some agents advertise their MCP clients as URL clients. Claude Code through
`claude-agent-acp` answers `initialize` with `mcpCapabilities: {http: true, sse: true}`;
protocol version 1 has no stdio flag, so the reply says nothing about stdio either
way. Jazyk takes the advertisement at its word: for such an agent it serves the
session's toolset over the MCP streamable HTTP transport, in its own process, and the
agent spawns nothing:

- One server per session, for the session's lifetime. It binds `127.0.0.1` on a port
  the operating system picks, and the `session/new` entry is an `http` MCP server:
  `{type: "http", name: "jazyk", url: "http://127.0.0.1:<port>/mcp", headers: [...]}`.
- A per-session bearer token guards it. The token is minted from the OS entropy
  source when the server starts and travels in the entry's headers as
  `Authorization: Bearer <token>`. A request without the header, or with another
  token, answers `401` and never reaches the handler; the refusal is a `note` in the
  transcript. Nothing else on the machine can call the serving, because nothing else
  holds the token.
- It speaks JSON-RPC 2.0 over `POST` to the one endpoint, the MCP streamable HTTP
  transport (protocol version `2025-03-26`; the `initialize` reply negotiates the
  version as the stdio serving does). A request answers `200`
  with `application/json` carrying the one response object; a notification answers
  `202` with no body; a batch array answers an array. `GET` and `DELETE` on the
  endpoint answer `405`: the serving opens no server-initiated stream and issues no
  `Mcp-Session-Id`, because one server is one session.
- Every request dispatches through the same handler the stdio loop uses: one
  registry, one set of schemas ([tools](../compiler/tools.md)), the same
  [bridge flags](#mcp-into-acp-sessions) (`--ephemeral`, `--only`, `--build-token`,
  `--packaged`, `--serve-files`), the same transcript under `<set-id>/trace/` in the [log directory](../compiler/jazyk-folder.md#logs). The
  batch lifecycle (`begin_goals`, the write tools, `mark_goal_done`, `done`) behaves
  identically, because it is the same code.
- Owned workers use the [compact task view](../compiler/task-runtime.md#results-and-verification).
  An owned worker's task history uses five-row pages with the existing cursors.
  The editor's HTTP task view and retained runtime evidence are unchanged.
- The server stops when the session closes. Jazyk closes the session first (the agent
  tears its client down), then stops the listener and runs the implicit finish, the
  same one end of input runs on a stdio serving: staged work commits under the
  budget path's gates, and the transcript finishes.
- A server that cannot start fails the session before it is created: the trace
  carries `sessionFailed` with `session: mcp http serving: cannot bind 127.0.0.1:
  <error>`, and the batch retries or fails as any session failure does. A session
  that started over HTTP says so in its trace with one `note`,
  `mcp serving: http on 127.0.0.1:<port>`. An agent that cannot reach the serving
  reports the failure through its own update stream, so it reads as tool rows
  carrying the agent's error, never as silence.

The stdio serving is not going anywhere: standalone agents connect to `jazyk mcp`
over stdio, the [IDE proxy](./acp.md#the-ide-proxy) injects a stdio entry, and the
embedded agent advertises no HTTP capability. HTTP exists for the session path and
only where the agent asks for it.

## The work loop

`tools/list` reflects the active batch. Before a claim, it retains the serving
catalog so clients can discover its lifecycle. After `begin_goals`, it advertises
the batch's toolset. Embedded native, ACP and browser runners refresh this list
before each model step. External clients can list again after a batch change.

A stdio serving tells its client when the list moves:

- `initialize` advertises `capabilities.tools.listChanged: true`. A serving over
  [HTTP](#mcp-over-http) advertises no `listChanged`: that transport carries no
  server-to-client messages.
- After the client's first `tools/list`, the serving compares the tool names it would
  list now with the ones the client last listed, after every message it answers. When
  they differ (`begin_goals`, `done`, `abandon_goals`, `init_project`, a set-local
  batch opening or closing), it writes
  `{"jsonrpc": "2.0", "method": "notifications/tools/list_changed"}` right after the
  response that moved them, and the names it compared become the new baseline.
- A client that never lists tools gets no notification.
File reads and the current-set matcher repair tools remain available during a
Bind or Generate batch when that serving supports them. Verify retains file reads
and listing without gaining file writes or matcher repair.
The same capability checks guard discovery and calls. The server's write gates
remain authoritative even when a client caches schemas.
A compile-only serving refuses a ledger batch whose tools it does not serve.
Bind and Generate use a generation serving. Verify uses a verification serving.

The agent-facing loop, common to the compile, generate, and verify toolsets:

Task queries accept an optional `entity` ID to inspect only that entity's rows.
Unknown IDs are errors, not empty successful results. The reply includes
`readyCount` for board-backed task queries. When no row is ready, its `next`
guidance describes the outstanding prerequisites instead of claiming a begin
operation can proceed. Release approval and compile prerequisites remain distinct.
An explicit verification `filter` lists ledger rows by derived status, including
findings without an open verify goal. These rows describe evidence, not readiness.
Omit `filter` to inspect the board and its `readyCount`.

1. Ask for work: `goals`, `binding_tasks`, `generation_tasks`, or
   `verification_tasks`. An empty answer is an answer, not an error; the board
   carries the build verdict when no goal is open.
2. Begin: `begin_goals` claims the next ready batch, the named batch, or the named
   goals as one batch; `begin_binding`, `begin_generation`, `begin_verification`
   claim one row each. The reply is the batch's contract: the assembled prompt as
   `instructions` and the loaded set as `package` for goals, the package for the
   rest.
3. Do the work: a goal batch stages graph writes and marks each goal
   `mark_goal_done` with a one-line justification (or `mark_goal_failed` with a
   reason); binding searches the deliverable and writes the missing test; generation
   edits deliverable files with the agent's own tools; verification runs or judges
   tests.
4. Finish: `done` runs the batch gates and commits the changeset, `record_binding`
   records the row, `record_generation` records the manifest, `run_tests` and
   `record_verdict` record verdicts. The reply names the next ready batch or row, so
   the loop chains without re-listing.

To watch for new work instead of polling, either run
[`jazyk monitor`](./cli.md#jazyk-monitor) as a background process and act on each
notice it prints (`--once` for a single blocking wake-up), or call `await_changes`:

- `await_changes({timeout_seconds?})`: a long poll. It returns when the graph's
  generation counter moves, a documentation file changes on disk, a manifest or test
  file in the deliverable changes, the ledger changes, or the
  [control plane](../compiler/control-plane.md) changes (a mode
  toggle or a release, so the user's click in the GUI is what wakes the agent), or
  at the timeout (default 300 seconds). The timeout is capped at 3600 seconds, and
  `timeout_seconds: 0` waits for the cap. The default returns because most MCP
  clients bound a tool call with their own timeout and would report a long block as
  an error; a client configured without that bound passes 0 and polls again when the
  call returns. A `notifications/cancelled` naming the call, or the end of input, ends
  the wait at once, and a cancelled call gets no response. The reply carries the changed documents,
  whether the graph is stale (documents changed but not yet reconciled), the board
  counts (open, blocked, parked, failed, per class), the open diagnostic counts, the
  pending generation work, the pending verification work grouped by reason, and the
  workflow modes with the gated counts.

### Watching edits

An agent edits project files with its own tools. Jazyk has no edit tool of its own
for them and needs none: the [dirty set](../compiler/reconciler.md#dirty-set) is
computed from content hashes on disk, so the next build sees every edit, whatever
wrote it. The serving watches so the agent sees the effect of an edit at once,
before any build.

- A native serving watches the project root with native file events from its first
  request until it exits. A file counts when it is `.jazyk/jazyk.toml` or a `docs`
  or `source` file of any document set. Its fingerprint is its size and modification
  time; an event that leaves every fingerprint unchanged records nothing, so editor
  temp files, renames, and Jazyk's own state writes never count.
- Every fingerprint change appends one edit with the next sequence number: the path
  and whether it was removed. An agent's edit tool, an editor, a formatter, and
  `git checkout` all count the same way.
- `await_changes` wakes on a recorded edit instead of waiting for its next poll,
  and lists edited `source` files in `changedDocs` beside the documents.
- `recent_edits({since?, setId?})`: the edits recorded after the cursor `since`
  (every edit since the serving started when absent), one row per path with its
  latest state. Each row names the sets and roles that select the path. A document
  row lists the compile goals the board derives on it (`place-anchors` on the
  document, `reconcile-section` on its sections), ready or awaiting a compile
  release, so the agent sees which sections its edit dirtied. A source row names
  the entity whose generation manifest lists the file (`owner`). The reply carries
  `cursor` (pass it as the next `since`), `graphStale` (such a goal stands in a set
  the rows name), and `watching`. Each call compares fingerprints itself before it
  answers, so an edit inside the event debounce still shows. `setId` narrows the
  rows to one set; a set-scoped worker defaults to its own set. The tool is read
  only: it never compiles, releases work, or writes the graph.
- Without native file events (`watching: false`, e.g. a hosted in-memory
  workspace), `recent_edits` still finds edits by comparing fingerprints between
  calls, and `await_changes` falls back to its 2 second poll.

E.g.:

```
recent_edits → {cursor: 3, watching: true, graphStale: true, edits: [
  {path: docs/orders.md, removed: false, sets: [{setId: ds_main, role: docs}],
   goals: [g:reconcile-section:docs/orders.md#/orders/refunds]},
  {path: src/orders.rs, removed: false, sets: [{setId: ds_main, role: source}],
   owner: ent:order}]}
```

The browser engine reports no edits and answers `watching: false`. An
`--ephemeral` serving neither watches nor serves `recent_edits`, as with
`await_changes`.

## The control plane over MCP

Writable native and hosted `chat` servings expose
`release_work({stage: "compile" | "generate", setId?})`. The tool records approval
for the selected set's current work and leaves workflow modes unchanged. The agent
uses it only for an explicit user request covering that stage. It refuses while
another build or an open graph batch holds the project. Other servings retain the
CLI and GUI release paths. A read-only chat cannot release work.

The serving is a worker among workers, and says so:

- `initialize` registers the serving in the
  [worker registry](../compiler/control-plane.md#workers-and-leases) under the client's
  name (kind `agent`), heartbeats while the process lives, and deregisters on exit.
- `begin_goals` takes the batch's lease; `done` and `abandon_goals` release it.
  `begin_binding` and `begin_generation` take the row's lease; `record_binding` and
  `record_generation` release it, and a row lease nothing records against expires
  by the lease TTL
  ([workers and leases](../compiler/control-plane.md#workers-and-leases)), so a
  row is never abandoned by a tool call, only recorded or left to expire.
  A batch another worker holds is refused with `claimed` naming the holder; a live
  internal build refuses every begin with `build-running` (compilation is
  sequential: one build, one session at a time). Any tool call on the open batch
  refreshes its lease.
- In `manual` mode a gated batch is refused with `awaiting-release`: the reply names
  the release that opens it (`jazyk release compile`, or the GUI). The board carries
  `gated` and `claimedBy` per goal, so an agent sees the whole board before
  claiming.

## Compilation over MCP

Binding and generation discovery report a missing document-set `source` selection
as setup work. They direct an explicitly requested generation workflow to
`list_document_sets`, then `update_document_set` with its configuration hash, then
the newly derived criteria goals. An empty source selection is not a finished
deliverable or a request for a nonexistent criteria batch.

Goal batches mutate the graph, so the serving holds an open changeset between calls,
exactly one at a time. The lifecycle has the same begin and done shape as every other
serving; what it claims is a batch:

- `goals({full?})` derives [the board](../compiler/reconciler.md#goal-derivation) from
  disk. The default reply is compact: the counts, the batches the scheduler would form
  (each with its id, its resolved [executor](./acp.md#executors), and its goals as
  id, kind, target, `mandatory`), one line per ready goal outside a batch, and the
  goals that are not ready summarized by kind and by blocking reason with their ids,
  never their hints. Past the first six batches only the shape travels: `moreBatches`
  lists the rest as id, locality, and goal counts by kind, and a blocking reason lists
  eight ids and a count of the rest, since a 41-batch board once answered with ninety
  thousand characters. A `countsNote` says which counts include optional goals (`open`,
  `ready`, `parked`, `failed` are the mandatory ones; `by_class` and `by_kind` include
  both), since a reader once tried to reconcile the block against itself. `full: true` returns every goal whole: kind, class, `mandatory`,
  target, `change`, `cause`, state, hints, [readiness](../compiler/reconciler.md#readiness)
  (`ready`, or the blocking reason as a sentence), `gated`, `claimedBy`, and for a parked
  goal `parkedReason`. A goal the
  calling serving itself holds (its open batch, or a batch of the running build the
  serving belongs to) reads `claimedBy: you`, and its blocking sentence says the goal
  is yours to mark and finish, since a session once read its own batch as another
  worker's claim and reported the board as inconsistent. The batches the reply lists are
  the ones the scheduler would form next, numbered past the ids live leases hold, so
  the batch a session is working on never shares an id with one the board would form
  now. The
  On a converged board that still lists optional batches, the note says how many
  optional goals stay claimable and `next` reads `optional: begin_goals claims <batch>
  or stop here`, since a reader once weighed the verdict against a line inviting a
  claim. The
  compact form exists because a thirty-goal board with forty-five gated bind goals
  once answered with seventy-five thousand characters, most of them hints on goals
  the agent could not claim. A batch id is `b<generation>-<n>`: the
  generation the board derives from and the batch's index within it. Batch ids are
  derived with the board, so a commit re-derives both and an id from an earlier
  generation names nothing. When no goal is open the reply carries the verdict with
  its counts instead.
- `begin_goals({batch?, goals?, full?})` claims the named batch, the named goals as
  one batch when they share a locality and their kinds' executors agree, or (with
  neither) the next ready batch (the highest ready tier, one locality, filled to the
  context budget: [batching](../compiler/reconciler.md#batching)). A `batch` the
  current board does not hold is refused, and the reply carries the current
  batches so the agent picks again without a second `goals` call. The claim reloads
  the store, syncs the section trees in memory, opens a changeset, assembles the
  [session prompt](../compiler/sessions.md#the-prompt) for the batch as
  `instructions` (ending in the
  [`serving-protocol.md`](../compiler/goals/prompts/session/serving-protocol.md.mustache) line, since
  the batch is already claimed when the agent reads it), and returns the batch id,
  its goals, and the status block of the initially
  [loaded set](../compiler/context.md#the-loaded-set) as `package` (the items' text
  travels inside `instructions`, under `## Loaded`, as in every session prompt). A
  [`place-anchors`](../compiler/goals/place-anchors.md) goal carries the document's
  alignment proposals in its loaded set; a
  [`reconcile-section`](../compiler/goals/reconcile-section.md) goal stays blocked
  while its document has pending alignment. The serving's toolset for the batch is
  the union of its goal kinds' [toolsets](../compiler/sessions.md#toolsets).
- The write tools stage into the changeset, validated by the same
  [gates](../compiler/graph.md#validation-gates) a session faces, scoped to the
  batch's locality the same way. A mutating reply previews the goals the mutation
  will open ([bubbling](../compiler/reconciler.md#bubbling)) and re-renders the
  condensed status block.
- `mark_goal_done({goal, justification, evidence?})` is validated against the goal
  kind's gate when staged and again at commit; a claim the gate refuses comes back
  with the gate named. `mark_goal_failed({goal, reason})` is always accepted.
  `load_skill({name})` brings a [skill](../compiler/sessions.md#skills) into the
  batch. The goal must belong to the open batch; a goal from elsewhere on the board
  is rejected toward `done`, because the next batch is where it gets claimed.
- Write tools and goal tools outside an open batch are rejected toward
  `begin_goals`. Identity, provenance, and scope rules hold because they are the
  same code path.
- `done({summary, goals?, beginNext?})` accepts final goal claims with the same
  `{goal, justification, evidence?}` shape and gates as `mark_goal_done`. It checks
  every inline claim before accepting the combined finish. A rejected claim or
  batch gate leaves none of this call's claims or staged effects behind. Earlier
  work remains available for repair. It runs the batch gates (every goal resolved or failed,
  coverage contract, stale anchors), commits atomically, writes the journal entry
  with `resolved_goals` and `opened_goals`, and re-derives the board. A gate failure
  leaves the changeset open and names the repair. The reply is `{committed, next}`,
  `next` naming the next ready batch by id with its goals, and a `ratificationNote`
  when the commit landed derived facts, saying how many ratify goals they opened and
  that they wait on a human (a session once watched the blocked count climb from 17 to
  46 across three criteria batches with no word why). `beginNext: true` claims
  that batch in the same call and carries its `instructions` and `package` in the
  reply, saving a round trip per batch; the default reply only names it. A `beganNote`
  beside the claim says that `abandon_goals` releases the batch untouched, since a
  session capped at a number of commits once found itself holding a batch it had not
  meant to take. The claim stops at convergence: when the commit leaves only optional goals ready, the reply
  carries the verdict and says so in words (`beginNext stops at convergence, so
  begin_goals takes the next batch when you want it`), since an agent told to converge
  once held an optional batch it had not asked for, and an agent that wanted it once
  read a bare "stay claimable" as a skipped claim. The first
  batch of a serving ships the agent contract and the active skills in full; later
  batches elide the contract and any skill already delivered (the agent saw them
  earlier in the same session), while the project block, the goals block, and the
  loaded block always ship whole. When that first reply would pass 45k characters, the
  skills ship as `load_skill` pointers instead of bodies and the reply opens with a
  line saying to load them, since a client once spilled a 58k reply to a file and
  spent four calls reading it back. A skill shipped as a pointer is not rendered:
  `load_skill` returns its text, since a session once heard that a skill it had never
  seen "already stands in the conversation". `begin_goals` with `full: true` repeats everything
  for a client that lost its context.
- A goal the batch neither resolved nor failed when `done` commits parks; a parked
  goal resumes first in the next batch that fits it
  ([parked and failed](../compiler/reconciler.md#parked-and-failed)).
- `abandon_goals({reason})` drops the staged work. An abandoned batch leaves no
  trace, the same contract as an aborted session; its goals return to `open`. A
  server that dies mid-batch loses only staging; any process recomputes the board
  and the goals reappear.
- GC goals become ready as their cones settle
  ([compile and garbage collection](../compiler/compilation.md#compile-and-garbage-collection)).
  The board offers them in the same order the internal scheduler would, so an
  external agent runs the same bursts: a GC goal whose locality matches the batch
  just committed is the next batch.
- When a commit leaves no open goal on the board (blocked and optional goals aside),
  the serving runs the deterministic tail itself
  ([checks](../compiler/compilation.md#checks), rendering, docsgen, verdict),
  because none of it needs a model. The final `done` reply carries the
  [verdict](../compiler/compilation.md#convergence) with its counts and, under
  `ready`, the generation rows that became ready (`ready: {generate: [...]}`);
  `generation` stays the counter in every reply. A verdict that counts blocked goals
  carries a `verdictNote` saying what they wait on (a `jazyk release` of the ledger
  work, a human answer) and that they do not hold the verdict, since a Codex session
  read "converged, 15 blocked" as a contradiction. A refused status write (the
  verdict, or the failed and parked goals a `done` recorded) carries `unsaved` with
  the refusal ([concurrency](../compiler/graph.md#concurrency)).

## Generation and verification over MCP

Binding and generation keep no staged graph batch: `begin_binding` and
`begin_generation` return the package, the agent searches, edits files, and runs
commands with its own tools, `record_binding` records the row and
`record_generation` the manifest. The binding package carries the requirement's
`statement` and `quote`. A package whose ledger holds no medium carries `mediumNote`,
and `record_medium` records the agent's decision before anything is written
([the medium](../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)). There is no batch in these lifecycles, so a graph write outside one is refused with a
note that names the channel: a contradiction the tests expose reaches the author
through the `failing-binding` check once the row is recorded, since a Codex session
generating a handbook tried `report_diagnostic` for one. A coding agent can bring its
own file tools. With `--serve-files`, the serving exposes the
[generation file tools](../compiler/goals/generate.md#file-and-command-tools).
Owned worker batches carry the execution contract in their opening prompt.
A batch containing generation also carries the complete generation contract there.
Its `begin_generation` replies omit the repeated instructions and retain the current
entity, facts, manifest and task data. Reopening another entity does not repeat the
contract. The serving records that delivery when its owned scope opens, before
any mutation. The protected MCP checkpoint retains it when board batches change
and across restoration. Binding-only batches keep the full binding contract in `begin_binding`
and omit the repeated execution contract. Standalone servings keep full instructions.
The latest successful `begin_generation` or `begin_binding` sets their ownership
target within that document set. A write renews that target's claim and refuses
another worker's live claim. Successful recording releases the claim and clears the
target; a rejected record keeps it available for repair. Editing a recorded entity
again starts with `begin_generation`, including a repair a human requested when no
generation goal is owed. A serving pinned to a goal keeps that goal's ownership scope.
Writable native and hosted chat enable these file tools by default. Reads and writes
remain inside the selected project's file and ownership guards. Execution uses saved
tasks; no host shell is exposed. Browser and read-only chat keep their smaller toolsets.
The `bind`, `generate`, and `verify` goals on the board are the same rows these
lifecycles claim: resolving a row resolves its goal. Each task row carries the goal's
`ready` flag and, when it is not ready, `blockedBy` with the board's reason; when every
row is gated the list's `next` names the release that opens them (`jazyk release
generate`), since a Codex session read a fully gated list and learned the command only
from the refusal that followed; `generation_tasks` carries a `staleNote` when rows
went stale since their last run (a shared test file or implementation file changed
under a generation), since their generate goals return only after `run_tests`
re-baselines them and a Codex session saw entities appear in the list it had read as
complete. An empty generation list does not assert that a deliverable exists.
Missing or stale requirement bindings open development work on their owning entity.
The agent creates source and tests together through `begin_generation` and records
their criterion declarations in the manifest. Binding remains an explicit lifecycle
for existing code and tests or deliberate test-first work. Discovery lists the work currently derived by the board.
It never substitutes for file inspection or verification. Also,
`begin_generation` refuses an entity whose goal is not ready (`not-ready`, the same
reason). Generation waits for graph and criterion readiness, then completes source
and current requirement bindings together
([readiness](../compiler/goals/generate.md#readiness)).

Verification: `run_tests` executes recorded programmatic tests (the build first, then
the commands) and records verdicts as a side effect. `llm` rows go through
`begin_verification` for the criteria and `record_verdict` for the judgment.

E.g. the loop after a docs edit:

```
await_changes → {changedDocs: [docs/orders.md], graphStale: true, board: {open: 2, blocked: 0}, ...}
goals → {generation: 412,
         goals: [{goal: g:place-anchors:docs/orders.md, kind: place-anchors, class: compile,
                  mandatory: true, ready: true},
                 {goal: g:reconcile-section:docs/orders.md#/orders/holds, kind: reconcile-section,
                  class: compile, mandatory: true, ready: false,
                  blocked: "alignment pending for docs/orders.md"}],
         batches: [{batch: b412-1, goals: [g:place-anchors:docs/orders.md], executor: embedded}]}
begin_goals {batch: b412-1}
  → {batch: b412-1, goals: [g:place-anchors:docs/orders.md],
     instructions: <the assembled prompt>, package: <section changes, proposals with candidates>}
(agent stages place_anchor / orphan_anchor, then mark_goal_done ...)
done → {committed: true, generation: 413,
        next: {batch: b413-1, goals: [g:reconcile-section:docs/orders.md#/orders/holds]}}
begin_goals {batch: b413-1}
  → {batch: b413-1, goals: [...], instructions: <...>,
     package: <the dirty section with its diff, its entities as stubs, stale anchors>}
(agent stages upsert_requirement / set_coverage, then mark_goal_done ...)
done → {committed: true, generation: 414,
        opened: [g:rejudge-pair:req:orders-6~req:payment-9, g:review-entity:ent:order],
        next: {batch: b414-1, goals: [g:rejudge-pair:req:orders-6~req:payment-9,
                                      g:review-entity:ent:order]}}
... judgment batches the same way; the cone settles and its GC goal is next:
done → {committed: true, generation: 415, next: {batch: b415-1, goals: [g:declare-edges:req:orders-6]}}
... last done → {committed: true, generation: 418,
                 verdict: {state: converged, blocked: 0, optional: 0},
                 ready: {generate: [ent:order]}}
binding_tasks → [{requirement: req:orders-6, reason: requirement-changed}]
begin_binding {requirement: req:orders-6} → package (statement, quote, entity, context)
(agent finds the test, sees it assert the old value, rewrites tests/orders.rs)
record_binding {requirement: req:orders-6, files: [src/orders.rs], test: {...}, verdict: fail}
  → row failing; ent:order is generation work
begin_generation {entity: ent:order} → package (carries the bound tests)
(agent edits src/orders.rs with its own tools until the bound test passes)
record_generation {entity: ent:order, factHash, manifest} → ledger updated
run_tests → build + commands run, verdicts recorded
```

A fix-fail-reverify cycle is self-terminating: editing a deliverable file re-stales
exactly the rows whose files hash moved, and the pending list shrinks monotonically
once tests pass. [`jazyk ripple docs/orders.md`](./cli.md#jazyk-ripple) replays the
whole cascade afterward, one line per journal entry.

## Transcripts

Every serving leaves a transcript: one JSON-lines file under `<set-id>/trace/` in the [log directory](../compiler/jazyk-folder.md#logs)
(`<ts>-mcp-cli<pid>.jsonl`; the file opens at server start, before the client
introduces itself, so the stem carries the process id, not the client name), one
`toolCall`/`toolResult`/`toolError` event per call
with condensed payloads, labeled by the open batch's id when one exists (`b412-3`)
and by the serving otherwise. The same format the compile trace writes
([trace events](../compiler/sessions.md#trace-events)), so an MCP session is
reviewable beside a build: what an agent asked, what it was told, and where it
stumbled.

## Reads and locking

Reads load the persisted graph from the out directory (see
[storage layout](../compiler/graph.md#storage-layout)). The server never compiles on
its own. If no graph exists yet, the compile toolset offers the board (a first build
is a board where every section is unprocessed); the other toolsets answer with
guidance to compile first. `report_feedback` answers either way: it writes only the
feedback log, never the graph.

Readers do not lock. They read the generation counter, load the node files, and retry if the
counter moved mid-read. Writers respect the store lock, so one changeset commits at a
time even with a compile running next to the server. See
[concurrency](../compiler/graph.md#concurrency).

## Stop fence

An internal worker serving accepts `--cancel-file PATH`. The runner creates this
attempt-specific file before stopping the worker. Once it exists, tool calls are
refused and end of input discards staged work. An ordinary session end still uses
the implicit finish. A subsequent build uses new session fences.

## Project integration context

Initialization names the project filesystem root used by the serving. This root
can differ from an external agent's working directory. Project file tools take
project-relative paths even when errors display absolute paths.

Every serving exposes read-only `project_context`, without a document-set ID.
The initialize instructions include the same context. ACP consumes these MCP
instructions too. Context identifies the project, repository URL, selected branch,
default branch, base commit, local changes, and available integration actions.
Unknown values remain null. Context never contains credentials or grants approval
to push, merge, dispatch, or deploy. Call `project_context` to refresh it.

Cloud supplies context from the authenticated project's current GitHub binding and
workspace. Native clients connected to Cloud use that same project-scoped adapter.
A local serving reads local Git metadata without contacting a remote. Each Git
query has a five-second limit, takes no optional locks, and never runs a
configured filesystem monitor. A query past its limit leaves its value null.
Browser-only projects report that no GitHub integration is attached. A metadata failure is
reported as unavailable, never as proof that the project is unlinked.
