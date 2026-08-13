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
  (`bootstrap/gui/dist`, committed). `--gui-dist DIR` (or `JAZYK_GUI_DIST`) serves from
  a directory on disk instead, for frontend development.
- Unknown non-API paths serve the app's `index.html`, so app routes are addressable
  URLs.
- Each start mints a random session token, embedded in the URL the browser opens. Every
  API and LSP request carries it (`Authorization: Bearer` or a `token` query
  parameter). Requests without it get `401`. The token keeps other local processes from
  spending LLM budget or writing documents through the API. `--no-token` disables the
  check, for frontend development.
- The app reads the token from the URL fragment once, stores it in `localStorage`, and
  drops the fragment so copied URLs do not carry the secret. Browser origins include
  the port, so each port keeps its own stored token: a reload, a new tab, or a browser
  restart on the same port resumes with the stored token, no fragment needed. A stale
  entry (the server restarted and minted a new token) surfaces as the token prompt,
  and a pasted token replaces the stored one.
- A `401` never fails quietly: the app blocks with a token prompt. This is the
  restarted-server case: a restart mints a new token, and an open tab's requests stop
  passing. Pasting the fresh URL (or just the token) into the prompt resumes the
  session in place, without a reload, so unsaved editor work in the tab survives. The
  prompt verifies the token against the API before it closes, then the event stream
  and the editor's WebSocket reconnect with it.
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
- `GET /api/feedback?limit=`: the [feedback log](../compiler/tools.md#feedback-tool),
  newest first, capped (default 200, maximum 2000). Each entry carries the model's
  report plus the references that name its caller. An unreadable or absent log is an
  empty list, never an error.

Documents:

- `GET /api/docs`: the matched documents with their content hashes, whether each is
  stale against the graph (on-disk hash differs from the reconciled hash), and its
  open diagnostics counted by severity. A diagnostic counts toward a document when a
  subject anchors there: a requirement whose source is the document, an entity with a
  mention in it, or a section reference into it. Suppressed diagnostics never count.
- `GET /api/docs/content?path=`: the raw document text and its hash.
- `GET /api/docs/baseline?path=`: the last reconciled text, reconstructed from the
  stored [section tree](../compiler/parsing.md) (sections in order, raw bodies
  joined). This is the diff baseline the editor marks changes against: the difference
  between it and the on-disk text is exactly what the next build's dirty set sees.
  `404` when the document has never reconciled.
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

Deliverable:

- `GET /api/deliverable`: the generated product as a file listing, each file with the
  entities and requirements the [ledger](../consumers/gen.md#the-ledger) binds to it,
  and the test artifacts pointing at it. Hidden entries, `target`, `node_modules`, and
  `jazyk-out*` directories are skipped, so a deliverable at the project root lists the
  product, not the compiler's output.
- `GET /api/deliverable/file?path=`: one file's text plus its resolved
  [sites](../consumers/gen.md#traceability): each ledger site whose `file` is this
  path, located against the current text (`exact`, `moved`, or `lost`), and each test
  whose artifact is this path, located by its embedded test name. Every entry names
  its requirement and current verification status. A file that is not text reports its
  size instead. Reads are confined to the deliverable directory.
- `GET /api/deliverable/baseline?path=`: the file as it stood before the last
  generation run rewrote it, from the snapshot generation takes at write time (see
  [incremental regeneration](../consumers/gen.md#incremental-regeneration)). `404`
  when generation never rewrote the file. Reads are confined to the baseline
  directory.

Diagnostics:

- `POST /api/diagnostics/{id}/triage` with `{triage}`: set the human
  [triage state](../compiler/model/diagnostic.md) (`acknowledged`, `suppressed`,
  `wontfix`, or null to clear). The write commits through the store as a journaled
  changeset.

## Jobs

The GUI runs builds and workers itself. `POST /api/jobs` with
`{kind: compile | gen | verify | audit | decompile}` (plus targets and `force` where
the kind takes them) queues a job and returns its id. `GET /api/jobs` lists jobs,
`GET /api/jobs/{id}` returns one job with its state, result, and its whole trace: the
server keeps every event a job emitted, numbered per job, so a reloaded page shows the
same history the live stream showed. `POST /api/jobs/{id}/cancel` requests
cancellation.

- Every job's trace persists as one JSON-lines file under `<out>/trace/`: a metadata
  line, one line per numbered event, and a final line with the outcome. The activity
  panel lists past jobs from these files, so the history survives server restarts and
  page reloads, and any tool can load a transcript programmatically. Files older
  than 30 days are removed when the server starts.
- The file holds the [full payloads](../compiler/turns.md#trace-events): every prompt
  sent and every reply received. What travels to the browser is elided: a string over
  2000 characters becomes a preview naming its full length, and every object holding
  one carries `elided: true`. `GET /api/trace/{stem}/{n}` returns event `n` of that
  transcript with nothing cut. Expanding
  a row in the activity panel is that fetch. A running job's events are readable the
  same way, by the same number, because the file is flushed per line.
- The metadata line and the outcome line each record the store generation at that
  moment, so a run's committed changesets are exactly the journal entries between
  the two. The activity panel renders them inline with the trace.
- Every build leaves a transcript, whichever frontend ran it: the CLI `compile`,
  `check`, `gen`, and `test` commands persist the same file (the metadata line carries
  `source: cli` and no job id), so the activity panel also lists builds that ran
  outside the GUI. See [CLI](./cli.md).
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

A dropped stream is shown, not hidden: the app banners the lost connection and falls
back to polling the status endpoint until the stream heals. Reconnecting refetches
every snapshot. When the drop turns out to be a rejected token (the polling answers
`401`), the banner gives way to the token prompt.

- `job.queued`, `job.started`, `job.trace`, `job.finished`: the job lifecycle.
  `job.trace` wraps one structured trace event.
- `chat.update`, `chat.permission`, `chat.sessions`: the [chat pane](#chat). One
  session update, one pending permission request, or a session list change.
- `store.lock`: the store lock appeared or disappeared. A build is starting or ending,
  in this process or any other.
- `store.generation`: the generation counter moved. Carries the new journal entries, so
  the client sees each committed changeset as it lands, mid-build included.
- `docs.changed`: matched documents changed on disk, with whether the graph is now
  stale.
- `pending.changed`: the generation or verification worklists changed size.
- `watch.state`: the workflow modes changed (compile or generation). Carries both.
- `control.changed`: the [control plane](../compiler/reconciler.md#the-control-plane)
  moved: a release landed, a worker registered or dropped, a lease was taken or
  freed. Carries the workers snapshot the workers strip renders.

External activity surfaces the same way: a `jazyk compile` run from a terminal, or an
[MCP](./mcp.md) agent committing through write tools, moves the lock and the counter,
and the GUI renders it live without owning the job.

## Layout

One workbench page. Navigation swaps panes, never the page. Six regions:

- The rail: a narrow icon strip on the far left: `files`, `graph`, `work`,
  `feedback`, `settings`. A rail item picks what the sidebar shows; it never
  navigates away.
- The sidebar: the navigator for the active rail item. Clicking an entry opens it in
  the center.
- The center: the open item. The document editor, the deliverable viewer, the map,
  the work views, the settings form.
- The inspector: the detail pane beside the center. Selecting a node anywhere (a code
  lens, a map node, a list row, an id chip) shows its detail here, beside the
  center, never replacing it. Closable; the center keeps its state under it.
- The chat pane: the persistent pane on the far right, collapsible to a strip. The
  conversation surface: chat sessions with the agent and follow views of automated
  work. See [chat](#chat).
- The activity panel: the bottom strip, always present. Collapsed it is one line:
  the run controls and the live build state. Expanded it is the run history and the
  selected run's transcript. See [activity](#activity).

Addressable state: `/files/docs/<path>`, `/files/deliverable/<path>`, `/graph`,
`/work`, `/feedback`, and `/settings` pick the center; `?node=` holds the inspector selection;
`?run=` the selected run. A document takes `?section=` and `?quote=` to reveal and
highlight a quote; a deliverable file takes `?site=<requirement>` to reveal that
requirement's first located site, or `?line=` to reveal a line directly. Routes from the earlier tabbed layout redirect to their new
homes.

### Files

One explorer over both trees, in two labeled sections:

- Documents: the docs tree. Each document shows its open diagnostics as a
  severity-colored badge and a drift dot when it is stale against the graph.
  Documents can be created, renamed, and deleted from the tree, through the
  documents API and its validation; a delete asks for a second click, never a
  dialog. Directories exist implicitly through paths.
- Deliverable: the generated product files, each with its ownership count badge
  (the entities and requirements the ledger binds to it) and a stale dot when a
  bound requirement's verification is stale.
- Build progress: while a build runs, the documents it is working on say so in
  place. A document queued in the current wave is dimmed with a waiting mark; the
  document a turn is reconciling shows a running mark, the section the turn reached,
  and how many of its dirty sections it has touched. When the turn ends, the row
  turns into its result (what was staged, or the failure) and fades a few seconds
  later. Hovering the row holds the result until the pointer leaves. The states come
  from the [trace events](../compiler/turns.md#trace-events) of the running job, so a
  build started outside the GUI moves the lock and the counter as always, but does
  not light the tree up: its events are in its own transcript, not on this server's
  stream.
- Linkage: the two sections light each other up. Selecting a document highlights
  the deliverable files bound to it: the requirements whose source anchors in the
  document, joined through the ledger to the files that implement them. Selecting
  a deliverable file highlights the documents its requirements anchor in. The
  highlight is a tint on the related rows, so the prose and the code it produced
  are visibly one system.

### Deliverable viewer

Opening a deliverable file shows it read-only in the center:

- Every resolved [site](../consumers/gen.md#traceability) shows as a code lens above
  its line (the requirement id and verification status). Clicking the lens opens
  the requirement in the inspector. Lost sites are flagged in the inspector.
- The viewer diffs against the file's generation baseline
  (`GET /api/deliverable/baseline`): gutter marks on the lines the last generation
  changed, and a `diff` toggle that swaps the viewer for a side-by-side diff of
  baseline against current text. A file with no baseline shows no marks and no
  toggle.

### Graph

The `graph` rail item is the whole graph surface: the sidebar navigates it, the
center draws it.

- The sidebar: one text filter plus facet lists, the viewer's cards served live:
  entities, requirements, diagnostics (with triage actions), coverage. Suppressed
  diagnostics never render. A row opens the node in the inspector and focuses it on
  the map.
- The center: the map. Nodes are typed: entities, documents, requirements, and
  deliverable files. Edges are typed too:
  - The [derived relationships](../compiler/graph.md#derived-data) between
    entities, drawn with UML notation: a hollow triangle for generalization, a
    hollow triangle on a dashed line for realization, a filled diamond for
    composition, a hollow diamond for aggregation, a plain line for association,
    an open arrow on a dashed line for dependency, a dotted line for reference.
  - A requirement to the entities it names (membership).
  - A requirement to the document its source anchors in.
  - A requirement to the deliverable files whose ledger sites implement it.
- Type chips filter which node types draw. The overview default shows entities and
  documents only: every requirement and file at once would drown the picture.
- Hidden types never break the picture: when two visible nodes are joined only
  through hidden ones, a collapsed tie draws between them, thin and dotted. A
  document connects to the files its hidden requirements implement, a document to
  the entities they name, an entity to its files, whatever the chip combination.
  Selecting the tie inspects the intermediary. With the intermediary type visible,
  the real edges carry the story and the collapsed tie disappears.
- Focus: selecting a node and focusing (or double-tapping it) pulls in every
  adjacent node of every type, chips notwithstanding: one neighborhood is never
  busy, so it shows everything, including the requirements and files the overview
  hides. Hops extend to 2 for the wider neighborhood.
- Selecting an edge lists the contributing requirements in the inspector.
- Entity scope and edge-type filters carry over from the overview.

### Work

The generation worklist and per-entity task packages; the verification matrix with
per-requirement status chips and the
[staleness cascade](../consumers/gen.md#the-cascade) explained per row. Rows open
the inspector. Run actions submit jobs to the activity panel.

### Feedback

The history of the [feedback tool](../compiler/tools.md#feedback-tool): what the
models reported about jazyk itself, newest first. This view is for jazyk's
developers, not for the project's authors; nothing here is a statement about the
documents.

- Each entry shows its kind, its subject, the message, and the references that name
  the caller: the source, the task, the target, the MCP client, the model, the codec,
  and the store generation.
- The kind is a filter: `?kind=` selects one, and the counts sit beside the filters.
- An entry made during a traced run links to that run, which selects it in the
  activity panel (`?run=`), so the call sits back in the transcript it came from.
- A feedback call mid-build refreshes the view as it lands, not at the end of the run.

### Inspector

The detail pane for one node, opened from anywhere, layered over nothing:

- An entity: name, definition, scope, mentions (each opens the editor at the
  quote), the requirements referencing it, its relationships, the files
  implementing it, and its verification rollup.
- A requirement: the EARS sentence, the source quote (opens the editor at the
  quote), its entities and edges, its implementing sites (each opens the
  deliverable file at the located line), and its verification status.
- A diagnostic: message, severity, subjects, reasoning, and the triage actions.
- Every node id anywhere in the app opens the inspector. The center never changes
  under it; the click-through from a requirement to its implementation is: open the
  inspector, then open a site.

### Chat

The chat pane is the GUI's [ACP client](./acp.md) surface. One session list, two
session kinds:

- Chat sessions: a conversation with the configured agent, created in the pane. The
  session gets the [`chat` serving](./mcp.md#toolsets), so the agent can read the
  graph, revise requirements through the
  [dual-write tools](./acp.md#dual-write-tools), run the task lifecycle, and edit
  project settings. Prompting streams the agent's thoughts, messages, and tool calls
  into the transcript as they happen.
- Follow sessions: every automated job turn ([jobs](#jobs)) registers as a read-only
  session, so watching a build is opening its session. The transcript is the same
  rendering as a chat session: the agent's messages, its tool calls, their results.

The pane's behaviors:

- [Slash commands](./acp.md#chat-sessions): `/compile`, `/generate`, `/verify`,
  `/status`, `/release`, completed in the prompt box from the advertised list. A
  command runs the real job and streams its progress into the same session.
- The [build plan](./acp.md#plans) renders as a live checklist: one entry per work
  item, flipping as the build advances.
- Follow mode: a toggle that pins the transcript to the newest update and moves the
  editor along with the work. A tool call carrying a location opens the document or
  deliverable file in the center at that line, so the center shows what the agent is
  touching while the pane shows what it is doing. Pair programming, with the agent
  driving.
- Permission requests from chat sessions surface inline as option buttons
  ([permissions](./acp.md#permissions)). An unanswered request cancels with the
  turn. Worker sessions never ask; their policy answers.
- Transcripts persist under `<out>/trace/` like job traces, so a reloaded page
  restores the session list and history.

API: `POST /api/chat/sessions` creates a session, `GET /api/chat/sessions` lists
them, `GET /api/chat/sessions/{id}` returns one with its transcript,
`POST /api/chat/sessions/{id}/prompt` sends a prompt (progress streams over the
event stream), `POST /api/chat/sessions/{id}/cancel` cancels the open turn, and
`POST /api/chat/permissions/{id}` answers a pending permission request. Updates
travel as `chat.update` events, elided like `job.trace`; permission requests as
`chat.permission`; session list changes as `chat.sessions`.

### Activity

The bottom panel merges what were the Build and Journal tabs: a run is one job plus
what it committed. Collapsed, the panel is a single control line; expanded, it is
two parts:

- The run list: newest first, live jobs and the transcripts on disk (CLI runs
  included), each with kind, state, timing, and its one-line result. Selecting a
  run pins it: a new job starting does not steal the view.
- The selected run: the transcript as turn groups, newest turn first, the running
  turn pinned and highlighted with its tool calls streaming in. A turn group is
  keyed by the event [label](../compiler/turns.md#trace-events), so parallel work
  reads as one group per document or entity, not as one interleaved stream. The
  header names what the turn is working on: the document, its dirty sections, and
  the section it reached.
- Inside a turn, one card per round. The card header is the round's arithmetic:
  prompt size, response time, completion tokens, and how many tool calls the answer
  produced. Expanding it shows the round in full, fetched on demand:
  - The request: every message in the order it was sent, each collapsible, the
    system prompt and the [context pack](../compiler/context.md) included. The pack
    is what the model was asked; nothing about it is inferred from the reply.
  - The response: the assistant message as it arrived, reasoning field included, and
    the parsed tool calls with their arguments.
  - The tool results the harness sent back, each with the full payload.
  A retry or a sticky fallback (codec downgrade, streaming, dropped `temperature`)
  shows as its own row in the round, with the error that caused it.
- The changesets the run committed (the journal entries whose build matches) render
  inline in order, each expandable to its mutations and reasoning: the trace says
  what the model did, the changesets say what landed.
- The control line, visible even collapsed: compile now (with the changed-document
  count), generate now (with the pending-entity count), verify, the
  [compile mode](#workflow-modes) select, and the generation mode select. The
  running job shows its kind and progress here; cancel is one click.
- The changeset timeline is still addressable per generation, and the release diff
  between any two generations stays reachable from the panel (the journal range
  diff).

## Benchmarks

The benchmarks tab grades and compares models
([benchmark](../benchmark/benchmark.md)):

- The table merges three sources, latest per model and codec: results embedded in the
  binary (`source: embedded`), the machine-wide history (`~/.jazyk/benchmarks/`), and
  the project's own `results.yaml`. Columns are the workflow verdicts, the four tier
  scores, efficiency, tokens, and throughput; rows with a different `caseSetHash` than
  the running binary's are marked stale.
- A run form: the endpoint URL (default: the resolved LLM settings), a model picked
  from the endpoint's `/v1/models` listing or typed free-form, and a run button that
  starts a benchmark [job](#jobs). Progress streams like any job; the finished grade
  lands in the table and the history.

API: `GET /api/benchmarks` (the merged table), `GET /api/benchmarks/models?baseUrl=`
(the endpoint's model listing), and `POST /api/jobs` with
`{kind: benchmark, baseUrl?, model?}` (the shared [jobs](#jobs) surface; returns the
job id).

## Workflow modes

The GUI always watches the documents (that is what `docs.changed` reports). What a
change triggers is the workflow mode, and the mode is not the GUI's private state: it
lives in the [control plane](../compiler/reconciler.md#the-control-plane)
(`control.yaml` in the out directory), where the internal loop, `jazyk monitor`, and
every agent over MCP read the same policy. A mode set in the GUI survives a restart
and binds the agents too; before the control plane, "manual" was a GUI-process
variable that no other worker could see, and a watching agent compiled on save
regardless.

`GET /api/watch` and `PUT /api/watch` carry `{compile, gen}`, each `auto` or
`manual`:

- `compile: manual` (the default): changes queue visibly and carry `gated: true`.
  The control line counts the documents that drifted from the graph; compiling is an
  explicit click. The click records a
  [release](../compiler/reconciler.md#modes-and-releases), so an attached agent's
  watcher fires from the same click.
- `compile: auto`: changes compile automatically, the loop of
  [`jazyk watch`](./cli.md#jazyk-watch): debounced events, a fingerprint gate,
  backoff retries for `incomplete` builds.
- `gen: manual` (the default): generation runs on click, which likewise records a
  release. The release covers [binding](../consumers/bind.md#when-binding-runs) too:
  owed bind tasks run before generation tasks.
- `gen: auto`: a finished compile with a non-empty
  [generation worklist](../consumers/gen.md#incremental-regeneration) queues a `gen`
  job behind it.

Decompilation has no mode: the decompile action is always an explicit click. It
records a decompile release for its scope and dispatches like compile and generate
([decompilation](../consumers/decompile.md#triggering)). The
[unclaimed report](../consumers/bind.md#the-unclaimed-report) beside the action shows
what territory has no docs; the count shrinks as drafts land and their statements
bind.

`--watch` starts with `compile: auto`. Automatic modes spend LLM budget, so both are
opt-in. With both automatic, a document change compiles and regenerates end to end;
the chain never loops, because generation does not touch the documents. Running
`jazyk watch` in a terminal beside the GUI is safe: commits serialize on the store
lock, and the second build finds nothing dirty.

### Workers

`GET /api/workers` reports the control plane: the modes, the registered
[workers](../compiler/reconciler.md#workers-and-leases) with their heartbeats and
held tasks, the live leases, and the gated task counts. The workers strip renders
it: who is attached ("claude-code agent, awaiting release", "working on reconcile
docs/orders.md"), and a release button per stage when gated work exists.

Compile and generate clicks dispatch by the `worker` preference
([dispatch](../compiler/reconciler.md#dispatch)): with an agent registered and
preferred, the click records the release and the agent does the work, its progress
streaming into the [activity view](#activity) from the MCP transcript; otherwise the
GUI runs its own job as before. `POST /api/release` with `{stage}` records a release
without running anything, the button the workers strip uses.

## Editor

The GUI embeds a code editor on the project's documents, backed by the language server
over `GET /lsp` (WebSocket, one JSON-RPC message per text frame, no Content-Length
framing). Each connection is its own session with its own open-document overlay. The
[capabilities](./lsp.md#capabilities) are the LSP's: anchored diagnostics, hover with
the rendered context pack, the requirement card, go to definition, references,
completion, document links, code lens.

- Markdown renders inline while editing. Headings take their size, emphasis and
  inline code take their style, links show their text, list bullets and quote bars
  draw as marks, and fenced code highlights in its own language. The markup syntax
  (`#`, `**`, backticks, `](url)`) appears only where the selection touches it, so
  the document reads like a page and edits like text. The text itself stays plain
  markdown, byte for byte: the docs are compiler input, provenance quotes locate
  against the exact characters, and nothing is rewritten by rendering.
- Document URIs are `file://` paths under the project root, as reported by
  `GET /api/project`.
- When a build commits, the server republishes diagnostics for every open document on
  every connection, the same refresh the [LSP](./lsp.md#rebuilds-and-refresh) does.
- Entity mentions are visibly marked in the text (a subtle accent underline), not
  only on hover, so what is clickable is discoverable. The marks come from the
  language server's document links.
- Requirement attachments show as the language server's [code
  lenses](./lsp.md#capabilities) above their quotes, so where a requirement anchors
  is visible without hovering. Clicking a lens opens the requirement in the
  inspector, beside the text.
- Hovering a requirement's quote shows the language server's
  [requirement card](./lsp.md#capabilities): the requirement, the code, and the test,
  each linked, with the verification status. The card's links stay inside the app: the
  requirement link opens the requirement in the inspector, a code or test link opens
  that deliverable file in the center at the line, and a link into another document
  opens the editor there. An `llm` test's criteria file has no page of its own, so that
  link lands on the requirement's verification detail in the inspector.
- Coverage renders beside the text from the section tree: covered, non-normative, and
  unprocessed sections are visually distinct.
- A build in progress is visible in the text. When a turn takes this document, its
  dirty sections are banded as queued, and the section the turn reached
  ([`section` events](../compiler/turns.md#trace-events)) is banded as running. Each
  band marks its first line in the gutter, beside the coverage border. When the turn
  ends, the bands become its result, green for committed and red for parked, and
  clear a few seconds later. Hovering a band holds it, and its tooltip names the
  turn, the section, and the outcome.
  Section lines come from the last reconciled section tree, the same source as the
  coverage bands, so they can drift against unsaved edits until the next build
  commits.
- The editor diffs against the reconciled baseline (`GET /api/docs/baseline`):
  changed, added, and deleted lines mark the gutter, updated live as the text
  changes. The marks answer what the next compile will see as dirty. A `diff`
  toggle swaps the editor for a side-by-side diff of baseline against current text;
  the current side stays editable. A never-reconciled document shows no marks and
  no toggle.
- Saving writes through the documents API with the conditional hash, so an edit made
  outside the GUI is never silently overwritten.
