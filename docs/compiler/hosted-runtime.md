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
programmatic test artifacts to `source`. Built artifacts and LLM judgment criteria
under the compiler output directory stay in the full snapshot. Unrecorded output
does not enter a delivery merely because it exists. A ledger record identifies
ownership, not a current verification verdict. Delivery still requires its own
approval and CI evidence against `criteria` and `graph_generation`.

These two maps contain UTF-8 text. A selected binary file returns an explicit
error. The full snapshot retains binary files and all internal state. Projection
never deletes a file from the workspace.

## Models and events

Each request receives an `Execution`. It names the model and supplies a transport
callback, cancellation flag, and optional trace callback. The transport accepts an
OpenAI-compatible chat request and returns a complete response. The host owns
provider authentication, accounting, call deadlines, and cancellation of an
in-flight transport. It must bound every call. The compiler checks cancellation
before and after calls and between agent turns.

Model capability fallbacks, token meters, and completion limits are private to an
execution. Hosted execution never reads the machine owner's provider settings or
environment credentials. A missing transport refuses model work. Read-only API
requests need no model. Trace callbacks are thread safe and carry the existing
[trace events](./sessions.md#trace-events).

## Execution profile

Compilation uses the in-process session runner shared with the browser. It invokes
the existing MCP tools directly and preserves their gates, identities, and journal.
Ledger sessions receive the same assembled batch prompt as native ACP sessions.
Their `begin_goals` acknowledgement is not the session prompt.
It never starts an ACP process or a customer command. Project executor settings
cannot enable subprocesses. Shell execution is unavailable. Programmatic tests
require delivery CI; they are not executed by this runtime. LLM verification can
use the supplied model transport. `POST /api/jobs` supports `compile`, `gen`,
`verify`, and `audit`. A hosted verification run skips the build command and returns
programmatic rows in `pendingCi` without recording a pass or fail. LLM rows judge
files already present in the snapshot. Source and test generation can edit virtual
files through the same tool gates; a command remains unavailable until delivery.

Hosted conversations use the existing in-process chat loop and MCP chat tools.
The snapshot stores their history and numbered updates in
`.jazyk/conversations.json`. Each new request reopens the loaded graph context and
instructs the model to reload handles. Conversation ids belong to the workspace.
Chat events use the editor's `chat.sessions` and `chat.update` shapes.

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
