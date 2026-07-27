# GUI

`jazyk gui [--port N] [--no-open] [--watch] [--gui-dist DIR] [--no-token]` starts one
local process: an HTTP server that serves the GUI web app, a JSON API over the
[graph store](../compiler/graph.md), a live event stream, and the
[language server](./lsp.md) over WebSocket. It then opens the browser at the served
address. The GUI is the workbench for developing a jazyk project: edit the documents,
watch the compiler reconcile, review the graph, review what changed, and run
[generation](../consumers/gen.md) and verification.

The GUI is the live superset of the [viewer](./viewer.md). The viewer stays the offline
snapshot; the GUI serves the same shards, updated as builds commit.

## Serving

- The server binds `127.0.0.1` only. It is never exposed beyond the local machine.
- Default port `4680`. When the default port is busy, the server binds an ephemeral port
  and prints the actual URL. An explicit `--port` that is busy is an error.
- `GET /api/ping` answers without a token: the app name, version, and the served
  project root. A server per project is normal, so when the default port is busy the
  startup line probes the occupant and names which project it serves. A stale tab is
  self-evident too: the app titles itself with the project directory and shows the
  root in the status bar.
- The web app is embedded in the binary at compile time from the built frontend
  (`bootstrap2/gui/dist`, committed). `--gui-dist DIR` (or `JAZYK_GUI_DIST`) serves from
  a directory on disk instead, for frontend development.
- Unknown non-API paths serve the app's `index.html`, so app routes are addressable
  URLs.
- Each start mints a random session token, embedded in the URL the browser opens. Every
  API and LSP request carries it (`Authorization: Bearer` or a `token` query
  parameter). Requests without it get `401`. The token keeps other local processes from
  spending LLM budget or writing documents through the API. `--no-token` disables the
  check, for frontend development.
- `POST /api/shutdown` and Ctrl-C stop the server immediately. Open browser
  connections (the event stream, the editor's WebSocket) never delay shutdown; they
  drop with the process. Shutdown cancels the running job and waits briefly for it,
  so no stale store lock is left behind.

## API

All endpoints live under `/api` and speak JSON. Reads load the store the same
[lock-free way](../compiler/graph.md#concurrency) every frontend does. Node ids resolve
through [redirects](../compiler/graph.md#mutations).

Project and store reads:

- `GET /api/project`: project root, out directory, docs glob, roots, deliverable
  directory, limits, and the LLM model and base URL. The API key is never served.
- `GET /api/status`: `status.yaml` plus node counts, the coverage fraction, and open
  diagnostics by severity. The same summary as `jazyk status`.
- `GET /api/graph`: every shard (entities, requirements, relationships, diagnostics,
  redirects) plus the generation counter.
- `GET /api/entities/{id}`: the entity, the requirements referencing it, its
  relationships, and its verification statuses.
- `GET /api/requirements/{id}`: the requirement and its verification status.
- `GET /api/search?q=`: the [search tool](../compiler/tools.md#read-tools).
- `GET /api/context?target=&focus=&budget=`: the rendered
  [context pack](../compiler/context.md) with its expansion handles.
- `GET /api/coverage`: per document, the section tree and the
  [coverage](../compiler/reconciler.md#coverage) map.
- `GET /api/journal?from=&to=&limit=`: [journal](../compiler/graph.md#journal) entries
  for a generation range, paginated, newest first.
- `GET /api/diff?from=&to=`: per-node before and after between two generations,
  reconstructed by replaying the journal. This powers the change review views. The
  journal entries between two builds are the release diff (see
  [release diffs](../consumers/pm.md#release-diffs-from-the-journal)).
- `GET /api/docsgen/{slug}`: the rendered per-entity requirements document.

Documents:

- `GET /api/docs`: the matched documents with their content hashes, whether each is
  stale against the graph (on-disk hash differs from the reconciled hash), and its
  open diagnostics counted by severity. A diagnostic counts toward a document when a
  subject anchors there: a requirement whose source is the document, an entity with a
  mention in it, or a section reference into it. Suppressed diagnostics never count.
- `GET /api/docs/content?path=`: the raw document text and its hash.
- `PUT /api/docs/content?path=` with `{text, baseHash}`: write the document. When the
  on-disk hash no longer matches `baseHash`, the write is rejected with `409` and the
  client re-reads. Paths are validated: inside the project root, matching the docs
  glob, never inside the out directory, no traversal, no symlink escape. A path that
  matches the glob but does not exist yet creates a new document.
- `POST /api/docs/rename` with `{from, to}`: move a document. Both paths pass the
  same validation; the target must not exist. The graph is not touched: the next
  build's dirty set sees the move, and the reconciler rewrites references
  mechanically.
- `DELETE /api/docs/content?path=`: delete the document. The graph is not touched:
  the next build reconciles the disappearance, and garbage collection removes what
  nothing mentions anymore.

Generation and verification:

- `GET /api/gen/pending`, `GET /api/gen/task/{entity}`: the
  [generation](../consumers/gen.md) worklist and the per-entity task package.
- `GET /api/verify/pending`: pending verification grouped by reason.
- `GET /api/verify/matrix`: every ledger row with its
  [derived status](../consumers/gen.md#status-is-derived-never-stored), plus rollup
  counts.

Settings:

- `GET /api/settings`: the [project settings](../compiler/project-settings.md) as
  parsed from `jazyk.toml`: which keys the file sets, the effective defaults for the
  rest, and the file hash. The API key is reported only as set or unset, never its
  value.
- `PUT /api/settings` with `{baseHash, settings}`: regenerate `jazyk.toml` from the
  form values and apply them to the running server without a restart. The write is
  conditional on `baseHash` like a document write. The file is rewritten in canonical
  form: comments do not survive, and a `redirect` or an `api_key` already in the file
  is carried over untouched. A file holding keys the form does not know is refused,
  so hand-maintained settings are never silently dropped; the editor still works on
  it.

Diagnostics:

- `POST /api/diagnostics/{id}/triage` with `{triage}`: set the human
  [triage state](../compiler/model/diagnostic.md) (`acknowledged`, `suppressed`,
  `wontfix`, or null to clear). The write commits through the store as a journaled
  changeset.

## Jobs

The GUI runs builds and workers itself. `POST /api/jobs` with
`{kind: compile | gen | verify | audit}` (plus targets and `force` where the kind takes
them) queues a job and returns its id. `GET /api/jobs` lists jobs,
`GET /api/jobs/{id}` returns one job with its state, result, and buffered trace
events. `POST /api/jobs/{id}/cancel` requests cancellation.

- Jobs run in-process, one at a time, in submission order. Every kind contends on the
  store lock and the LLM budget, so serializing them is the point.
- Submitting a `compile` while one is already queued returns the queued job's id.
- Cancellation is best effort: a job stops at its next boundary (between waves,
  entities, or rows). An LLM call already in flight is not interrupted. A cancelled
  compile parks its remaining work, and the next build resumes it.
- Job progress streams as [trace events](../compiler/turns.md#trace-events) over the
  event stream, the same events the `compile` command renders as its live trace.

## Events

`GET /api/events` streams server-sent events. Every event carries a monotonic sequence
number; the server keeps a replay ring, so a reconnecting client resumes from its last
seen event. When the gap exceeds the ring, the server sends `resync` and the client
refetches its snapshots.

- `job.queued`, `job.started`, `job.trace`, `job.finished`: the job lifecycle.
  `job.trace` wraps one structured trace event.
- `store.lock`: the store lock appeared or disappeared. A build is starting or ending,
  in this process or any other.
- `store.generation`: the generation counter moved. Carries the new journal entries, so
  the client sees each committed changeset as it lands, mid-build included.
- `docs.changed`: matched documents changed on disk, with whether the graph is now
  stale.
- `pending.changed`: the generation or verification worklists changed size.
- `watch.state`: the watch mode changed.

External activity surfaces the same way: a `jazyk compile` run from a terminal, or an
[MCP](./mcp.md) agent committing through write tools, moves the lock and the counter,
and the GUI renders it live without owning the job.

## Editor

The GUI embeds a code editor on the project's documents, backed by the language server
over `GET /lsp` (WebSocket, one JSON-RPC message per text frame, no Content-Length
framing). Each connection is its own session with its own open-document overlay. The
[capabilities](./lsp.md#capabilities) are the LSP's: anchored diagnostics, hover with
the rendered context pack and verification summary, go to definition, references,
completion, document links.

- Document URIs are `file://` paths under the project root, as reported by
  `GET /api/project`.
- When a build commits, the server republishes diagnostics for every open document on
  every connection, the same refresh the [LSP](./lsp.md#rebuilds-and-refresh) does.
- Entity mentions are visibly marked in the text (a subtle accent underline), not
  only on hover, so what is clickable is discoverable. The marks come from the
  language server's document links.
- Coverage renders beside the text from the section tree: covered, non-normative, and
  unprocessed sections are visually distinct.
- Saving writes through the documents API with the conditional hash, so an edit made
  outside the GUI is never silently overwritten.
- The document tree is a small file explorer. Each document shows its open
  diagnostics as a severity-colored badge and a drift dot when it is stale against
  the graph. Documents can be created, renamed, and deleted from the tree, through
  the documents API and its validation; a delete asks for a second click, never a
  dialog. Directories exist implicitly through paths.

## Watch

The GUI always watches the documents (that is what `docs.changed` reports). What a
change triggers is the watch mode: `GET /api/watch` and `PUT /api/watch` with
`{mode}`, one of:

- `off`: changes update the document badges and nothing else.
- `queue` (the default): changes queue visibly. The status bar counts the documents
  that drifted from the graph and lists them on demand; compiling stays an explicit
  click. The queue is derived, not stored: a document is queued while its on-disk
  hash differs from the reconciled hash, so a commit drains it and an external build
  drains it too.
- `watch`: changes compile automatically, the same loop as
  [`jazyk watch`](./cli.md#jazyk-watch): debounced events, a fingerprint gate, and
  backoff retries for `incomplete` builds. Changes during a running build queue one
  follow-up compile.

`--watch` starts in `watch` mode. Compiling spends LLM budget, so the automatic loop
is opt-in. Running `jazyk watch` in a terminal beside the GUI is safe: commits
serialize on the store lock, and the second build finds nothing dirty.

## What it shows

- Home: the build stats, the attention list (pending generation, pending verification,
  top open diagnostics), recent changesets, and the run actions.
- Docs: the document tree and the editor, with per-document diagnostic and coverage
  badges.
- Build: the running job with its waves, turns, tool calls, and the model's reasoning,
  live; past jobs with their reports.
- IR: the graph browser, the viewer's cards served live: entities, requirements,
  relationships, diagnostics (with triage actions), coverage. One text filter plus
  facets. Suppressed diagnostics never render.
- Map: the entity graph drawn as nodes and edges. Nodes are entities; edges are the
  [derived relationships](../compiler/graph.md#derived-data), drawn with UML
  notation: a hollow triangle for generalization, a hollow triangle on a dashed line
  for realization, a filled diamond for composition, a hollow diamond for
  aggregation, a plain line for association, an open arrow on a dashed line for
  dependency, a dotted line for reference. Selecting an edge lists the contributing
  requirements. Filters cover scope, edge type, and neighborhood focus.
- Journal: the changeset timeline, one changeset per generation with its work item,
  mutations, and reasoning; and the release diff between any two generations.
- Settings: the project settings as a form: the docs glob, the roots, the deliverable
  directory, the LLM endpoint and model, the lint rules, and the limits, each with
  its effective default when unset. Saving rewrites `jazyk.toml` and applies live.
- Work: the generation worklist and per-entity task packages; the verification matrix
  with per-requirement status chips, the
  [staleness cascade](../consumers/gen.md#the-cascade) explained per row, and run
  actions.

Every node id links to its detail view, every source reference opens the editor at the
located quote, and the verification chips use the same statuses and colors as the
[viewer](./viewer.md#verification-overlay).
