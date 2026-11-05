# Hosted runtime

The native library exposes `hosted::Workspace` for a service that owns project
storage, accounts, subscriptions, and scheduling. It uses the same project loader,
graph store, API operations, goal gates, and reconciler as the CLI and browser.
The service is separate from the engine. A local installation needs no service.

## Workspace snapshots

A workspace is a map from project-relative file paths to bytes. It contains source
documents, `jazyk.toml`, the graph, human decisions, generation ledger, and other
project files. Paths must be normalized relative paths. Absolute paths, parent
traversal, backslashes, colons, null bytes, and empty path components are rejected.

`Workspace::new` validates a snapshot. `Workspace::request` handles the shared
editor API synchronously. `Workspace::snapshot` exports the current snapshot.
Replies contain an HTTP status, content type, and bytes. JSON routes retain the
editor API response shape. File routes can return binary data.

Every request installs a private virtual filesystem rooted at `/project` on its
calling thread. Other threads and other workspaces cannot see it. All reads and
writes stay under that root. The request restores its previous thread context on
return and during panic unwinding. It does not change the process working directory.

The host gives each project one writer and persists the exported snapshot with a
revision check. An ordinary failed or blocked build can include valid committed
changes. The host saves that snapshot and reports the compiler verdict. A panic
rolls back the entire request snapshot and returns a distinct failure. Staged
changes still pass the existing commit gates before they enter a snapshot.

A hosted workspace keeps no per-batch replay state. A batch writes no
[graph snapshot](./sessions.md#snapshots) and no trace transcript. Every request
removes `<out>/snapshots/` for each document set, so a workspace holding snapshots
sheds them on its next request.

## Source and delivery projections

`Workspace::projection` takes the previous authored file paths. It returns `source`,
`files`, `criteria`, and `graph_generation`. Paths are workspace-relative. The
operation is read-only and uses the same project, graph, and generation ledger
loaders as compilation.

`source` keeps authored files that still exist, plus the resolved `jazyk.toml`
files and documentation inputs. Deleted files disappear. Graph state, conversations,
compiler backups, Git internals, and ledger-owned generated files are excluded.
The host records new authored membership when a person imports or creates a file.

`files` adds existing ledger-recorded implementation files, support files, and
programmatic test artifacts to `source`. It also adds each set's committed compiler
state: the files the generated `.jazyk/.gitignore` keeps
([committed and local state](./graph.md#committed-and-local-state)), the LLM judgment
criteria under `gen/` among them. A repository that receives `files` can be
checked out and compiled incrementally. Built artifacts and local compiler state
(logs, diagrams, derived kinds, the journal, the control plane) stay in the full
snapshot. Unrecorded output does not enter a delivery merely because it exists. A ledger record identifies
ownership, not a current verification verdict. Delivery still requires its own
approval and CI evidence against `criteria` and `graph_generation`.

These two maps contain UTF-8 text. A selected binary file returns an explicit
error. The full snapshot retains binary files and all internal state. Projection
never deletes a file from the workspace.

## Models and events

Each request receives an `Execution`. It names the model and supplies a transport
callback, cancellation flag, and optional trace callback. The async transport
accepts an OpenAI-compatible chat request and returns a complete response. An
optional streaming transport also delivers text and reasoning deltas. The host owns
provider authentication, accounting, call deadlines, and cancellation of an
in-flight transport. It must bound every call. The compiler checks cancellation
before and after calls and between agent turns.

The host can set `Execution.pause_reason` before raising the cancellation flag to
stop work for a provider or resource failure. Chat records `turn_end` with
`stop: paused` and that reason. A user cancellation without a pause reason stays
`cancelled`. The distinction survives snapshot storage and reopening the conversation.

Token meters and completion limits are private to an execution. Native tool calling
is required. Hosted execution never reads the machine owner's provider settings or
environment credentials. A missing transport refuses model work. Read-only API
requests need no model. Trace callbacks are thread safe and carry the existing
[trace events](./sessions.md#trace-events).

## Execution profile

Compilation uses the in-process session runner shared with the browser. It invokes
the existing MCP tools directly and preserves their gates, identities, and journal.
Ledger sessions receive the same assembled batch prompt as native ACP sessions.
Their `begin_goals` acknowledgement is not the session prompt.
It never starts an ACP process or a customer command on the host. Project executor
settings cannot enable host subprocesses. An injected task runtime runs saved finite
tasks in isolated containers. Programmatic tests and builds use `task:<id>` and the
same report and product import rules as local execution. Without an injected runtime,
programmatic rows remain in `pendingCi` without a pass or fail. LLM verification uses
the supplied model transport and files in the snapshot. `POST /api/jobs` supports
`compile`, `gen`, `verify`, and `audit`. Source and test generation edit virtual files
through the same tool gates. See [task runtimes](./task-runtime.md).

Hosted conversations use the existing in-process chat loop and MCP chat tools,
including guarded project file tools and the scoped `release_work` action for
explicitly requested compilation or generation.
The snapshot stores their history and numbered updates in
`.jazyk/conversations.json`. Each new request reopens the loaded graph context and
instructs the model to reload handles. Conversation ids belong to the workspace.
Chat events use the editor's `chat.sessions` and `chat.update` shapes.

Hosted conversations receive the project's
[agent context](../frontends/agent-context.md) from the snapshot: instruction files
and skills. The host's global layer and its remote MCP servers arrive through
`Execution.agent_context` ([hosted context](../frontends/agent-context.md#hosted-context)).
The engine opens no connection for a remote server and never runs a skill script.
The host creates the
[jazyk section of `AGENTS.md`](../frontends/agent-context.md#the-jazyk-section-of-agentsmd)
in a new project with `Workspace::sync_agents_md(true)` and refreshes it with
`Workspace::sync_agents_md(false)` before a writable job. Configuration writes
inside the engine refresh it as they do natively.

The hosted profile does not read the host filesystem, global configuration, or
external renderer configuration. Diagram rendering stays in process. It uses the
renderer's parser, layout, and SVG stages directly, with no filesystem preprocessor. Image markup and
preprocessor directives are refused. SVG export is available; PNG rasterization
is unavailable in the hosted profile because its font and image loader uses the
host filesystem. It starts no lease heartbeat threads. The host owns the lifetime of a running operation.

The native CLI, ACP, LSP, and local GUI retain their normal filesystem and process
behavior outside this profile. The browser retains its existing memory filesystem
and model bridge. See [the filesystem seam](../frontends/wasm.md#filesystem) and
[editor protocol](../frontends/protocol.md).

## Conversation input and model deltas

`Execution.inputs` drains ordered accepted inputs for one conversation at safe
agent-loop boundaries. `Execution.input_close` atomically closes admission only
when no input remains. A failed close returns to input delivery. Hosted adapters
fence these callbacks to the current job owner and preserve undelivered messages
across pauses and worker failure. Input delivery events follow history insertion.

A manually started job can name its owning `conversationId` and numeric `jobId`.
The conversation must exist and have no running conversational turn. Its transcript
records a `workflow` update with that job identity. The update survives workspace
snapshots. A worker batch drains inputs for its owning conversation through the
same input callback. It records delivery after inserting the text into worker
history. It keeps the batch's tools, goals, and commit gates. Steering does not
authorize unrelated writes or a second writer.

An owned job accepts the same `afterN` event watermark as a conversational prompt.
Before appending its workflow update, it advances the conversation's next event
number past that watermark. The host supplies the durable event cursor when an
older workspace snapshot may trail events from a failed prior worker.

Admission remains open between worker batches. Ending a batch does not call
`Execution.input_close`. The host closes admission when the whole job ends and
preserves any undelivered input. A job without a conversation keeps its existing
non-conversational execution.

Portable capabilities advertise `conversationJobs: true`. The native GUI does not
advertise it. Its jobs retain their separate build transcript until its ACP job
runner exposes the same ownership and input delivery contract.

`Execution.transport_stream` optionally delivers `content`, `reasoning_content`,
or `reasoning` deltas while returning the complete model response for history and
tool validation. A body-only transport remains supported. Streamed tool arguments
never execute before a complete validated response. The host bounds and cancels
model calls and coalesces small deltas before persistence.

## Durable execution

Hosted execution supplies a versioned resume capsule, a suspension signal distinct
from Stop, an admitted asynchronous model transport and an asynchronous checkpoint
callback. The callback receives owned execution state and the scoped workspace
snapshot. It validates and advances the execution sequence before returning.

The [shared runtime](./agent-runtime.md#checkpoints) defines model and tool receipts,
MCP restoration and parent continuation. The private host owns storage, tenant
policy, job reclaim and idempotent accounting reconciliation. Internal transport
metadata identifies model requests and is removed before provider dispatch.
