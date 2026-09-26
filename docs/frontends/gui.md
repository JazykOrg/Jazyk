# GUI

`jazyk gui [--port N] [--no-open] [--watch] [--gui-dist DIR] [--no-token] [--dev]` starts one
local process: an HTTP server that serves the GUI web app, a JSON API over the
[graph store](../compiler/graph.md), a live event stream, and the
[language server](./lsp.md) over WebSocket. It then opens the browser at the served
address. The GUI is the front door of a jazyk project: read and edit the documents,
see what the compiler understood and what it asks, walk the model and its diagrams,
review the product against the documents, and talk to the agent. The compiler's own
workbench (the board, the runs, the prompts) sits behind the
[Developer toggle](#developer-toggle). `--dev` starts with the toggle on.

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
  (`bootstrap/app/dist`, committed). `--gui-dist DIR` (or `JAZYK_GUI_DIST`) serves from
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
  directory, the [executors](../compiler/project-settings.md#executors), and the LLM
  model and base URL. The API key is never served.
- `GET /api/status`: `status.yaml` (the store version, the generation, the
  [verdict](../compiler/compilation.md#convergence) with its counts, the costs) plus
  node counts, the coverage fraction, open diagnostics by severity, and the board
  counts. The same summary as `jazyk status`.
- `GET /api/overview`: the same summary without the board counts, plus verification
  counts by [derived status](../consumers/gen.md#status-is-derived-never-stored).
  The viewer-style rollup, served live.
- `GET /api/graph`: every shard (entities, requirements, criteria, views, relationships, state
  machines, diagnostics, redirects) plus the generation counter.
- `GET /api/entities/{id}`: the entity, its parent and children, its attributes, the
  requirements referencing it, its relationships, the views it belongs to, its derived
  state machine when one exists, and its verification statuses.
- `GET /api/requirements/{id}`: the requirement (statement, entities, edges,
  transition, facets, provenance) and its verification status.
- `GET /api/views`: every [view](../compiler/model/view.md), default and curated, with
  kind, title, member count, edge count, and where it stands against the
  [limits registry](../compiler/graph.md#limits).
- `GET /api/views/{id}`: one view resolved for drawing: the members in order, the
  excluded members with their notes, the query matches, the collapse set, and every
  relationship among the members with lifting applied (one arrow per direction-and-type
  group, the concrete edges beneath each lifted arrow, each with its requirements). A
  flow view carries its ordered steps with their participants; a state view carries
  the derived machine. The `children` list names, for every member that has a
  [level view](../compiler/diagrams.md#level-views) of its own, the member id and that
  view's id: the same list `get_view` answers
  ([drill-down](../compiler/concepts/levels.md#drill-down)). This is what the map
  draws. `?detail=N` is the [explorer's](#explore) detail: every drawn entity's
  children join the members N levels down, each marked `detail` with the level it
  came in at and `parent` set, and the arrows lift to the wider set by the same
  code. The reply's `detail` is the number of levels applied (the expansion stops
  where the tree ends) and `deeper` says whether one more level would draw
  anything.
- `GET /api/entities/{id}/card`: the entity's [card](../consumers/docsgen.md#entity-cards)
  as JSON, from the shared model: `id`, `name`, `stereotype`, `definition`,
  `provenance` (`quote`, `derived`, or `decree`), `breadcrumb` (the chain from the
  scope root, id and name each), `context` (the parent level's structural view id),
  `inside` (the entity's own level view id, or null), `insideFlows` (the flow view ids
  of its level), `relationships` (`{other, type, direction, count}`), `flows` (the
  flow view ids at the parent's level it takes part in), `siblings` and `children`
  (`{id, name, childCount}`), `requirementCount`, and `proposal` (the pending
  ratification diagnostic id, or null).
- `GET /api/views/{id}/page`: the view's [diagram page](../consumers/docsgen.md#diagram-pages)
  as JSON: `id`, `kind`, `title`, `level` (the level target and its breadcrumb),
  `drawn` (`{id, name, stereotype, levelView}` in drawing order), `steps` (flow kinds:
  `{requirement, statement, from, to}`), and `around` (`{sameLevel, above, below}`
  as view ids).
- `GET /api/tree`: the containment tree, one node per scope root (`scope:<scope>`)
  and per entity, each with its children in document order, its child count, and
  its level view id when it has one (`null` on a leaf). The tree panel and the
  breadcrumb draw from it ([levels](../compiler/concepts/levels.md#the-scope-root)).
- `GET /api/search?q=`: the [search tool](../compiler/tools.md#read-tools).
- `GET /api/context?target=&depth=`: what [`load`](../compiler/context.md#tools)
  renders for the target: the loaded set of that one load, with its expansion handles.
- `GET /api/coverage`: per document, the section tree and the
  [coverage](../compiler/compilation.md#coverage) map.
- `GET /api/journal?from=&to=&limit=`: [journal](../compiler/graph.md#journal) entries
  for a generation range, paginated, newest first. Each entry carries its
  `resolved_goals` with their justifications and its `opened_goals` with their causes.
- `GET /api/diff?from=&to=`: per-node before and after between two generations,
  reconstructed by replaying the journal. `to` past the current generation is
  clamped to it; `from` below 1 or above `to` is `400`. This powers the change
  review views. The journal entries between two builds are the release diff (see
  [release diffs](../consumers/pm.md#release-diffs-from-the-journal)).
- `GET /api/docsgen/{slug}`: the rendered per-entity requirements document.
- `GET /api/feedback?limit=`: the [feedback log](../compiler/tools.md#feedback-tool),
  newest first, capped (default 200, maximum 2000). Each entry carries the model's
  report plus the references that name its caller. An unreadable or absent log is an
  empty list, never an error.
- `GET /api/capabilities`: the backend descriptor the app renders against
  ([backends and capabilities](#backends-and-capabilities)). The server answers with
  everything it serves; another backend answers with what it can do.
- `GET /api/projects`: the projects this server has opened, newest first, each with
  its root, its shape (documents only, documents and product, product only), and its
  status summary ([home](#home)).
- `GET /api/inbox`: the [Inbox](#inbox) rollup: the counts of open questions,
  pending proposals, open problems by severity, documents awaiting a release, and
  drafts, plus the sum the top bar shows.

The board and causality:

- `GET /api/board`: the [board](../compiler/reconciler.md#goal-derivation) as the
  reconciler derives it from disk: every open, blocked, parked, and failed goal with its
  kind, class, `mandatory`, target and unit, `change`, `cause`, `state`, hints, its
  [readiness](../compiler/reconciler.md#readiness) (ready, or the blocking reason as a
  sentence), and the batch id when a running session holds it. The ready goals come
  grouped into the batches the scheduler would form ([batching](../compiler/reconciler.md#batching)),
  each with its id (`b<generation>-<n>`), so the board shows what the next session
  takes before it starts. Counts by class, kind, and state ride beside the list, and
  the verdict when the board is empty. The same board `jazyk status` counts and
  `goals({})` lists [over MCP](./mcp.md#compilation-over-mcp).
- `GET /api/preview?goal=`: the next session's prompt exactly as the model receives it
  ([preview](../compiler/sessions.md#preview)), plus the batch's toolset and the
  executor it resolves to. With `goal=`, the batch that goal would join. In
  `compile: manual` before a release, the gated goals form no batch yet, so the
  reply carries the batch the release would form, with `gated: true` and a note
  saying so. A preview makes no LLM call and spends nothing.
- `GET /api/explain?target=`: for a goal id, the change record that produced it, what
  its readiness says, and what blocks it; for a node id or a section reference, the
  cone of goals a change to it would open. The same rendering as `jazyk explain`.
- `GET /api/ripple?generation=`: the ripple DAG rooted at a generation, forward: the
  goals that generation opened, the generations that resolved them, and so on, each
  step with its cause and justification. `target=` instead of `generation=` roots the
  DAG at the last cascade that touched a node; `back=true` walks causes instead of
  consequences. A generation's DAG doubles as the whole-build report: the cost beside
  it, and the parked and failed goals with their reasons. The DAG is computed over the
  journal, never stored.

Documents:

- `GET /api/docs`: the matched documents with their content hashes, whether each is
  stale against the graph (on-disk hash differs from the reconciled hash), its open
  diagnostics counted by severity, and its open goals counted by kind. A diagnostic
  counts toward a document when a subject anchors there: a requirement whose source is
  the document, an entity with a mention in it, or a section reference into it.
  Suppressed diagnostics never count.
- `GET /api/docs/content?path=`: the raw document text and its hash.
- `GET /api/docs/baseline?path=`: the last reconciled text, reconstructed from the
  stored [section tree](../compiler/parsing.md) (sections in order, raw bodies
  joined). This is the diff baseline the editor marks changes against: the difference
  between it and the on-disk text is exactly what the next build's
  [dirty set](../compiler/reconciler.md#dirty-set) sees. `404` when the document has
  never reconciled.
- `PUT /api/docs/content?path=` with `{text, baseHash}`: write the document. When the
  on-disk hash differs from `baseHash`, the write is rejected with `409` and the
  client re-reads. Paths are validated: inside the project root, matching the docs
  glob, never inside the out directory, no traversal, no symlink escape. A path that
  matches the glob but does not exist yet creates a new document. A save that changes
  sections is the root of a ripple: the next build journals it as an `edit` entry
  ([journal](../compiler/graph.md#journal)) and derives goals from it.
- `POST /api/docs/rename` with `{from, to}`: move a document. Both paths pass the
  same validation; the target must not exist. The graph is not touched: the next
  build's dirty set sees the move, and the reconciler rewrites references
  mechanically.
- `DELETE /api/docs/content?path=`: delete the document. The graph is not touched:
  the next build reconciles the disappearance, and
  [garbage collection](../compiler/graph.md#garbage-collection) removes what nothing
  mentions anymore.

Facts:

- `POST /api/facts/{id}/edit` with `{field, value, note?, proposal?}`: edit one fact
  through the graph, the [edit paths](../compiler/compilation.md#edit-paths) the
  inspector uses. `id` is a node id; `field` is a field of it (`definition`, an
  attribute value, `statement`, an edge's `type` or `cardinality`, `transition`, a
  view's `members`).
  - A quote-provenanced fact without `proposal`: nothing commits. The server asks the
    model for the sentence rewrite behind the fact and answers with
    `{proposal: {doc, section, old_text, new_text}}`.
  - The same call with the `proposal` echoed back: the dual write commits, the graph
    mutation and the prose replacement in one changeset. The commit absorbs the new
    section hashes, so the edit does not dirty the document it just changed. `409`
    when the document changed on disk since the proposal, or when the editor holds
    unsaved edits to it; save first, then retry.
  - `{decree: true}` on a quote-provenanced fact, or any edit of a derived or decree
    fact: the edit lands graph-only with `decree` provenance and queues a
    [ratification proposal](../compiler/model/diagnostic.md#ratification-proposals).
    The reply names the generation and the `ratification-pending` diagnostic.
  - `field: limits.<limit>` with a positive `value`: the node's own threshold. The
    server stages [`bump_limit`](../compiler/graph.md#mutations), not `edit_fact`:
    `limits: {<limit>: value}` lands on the entity or view with decree provenance in
    the journal ([per-node bumps](../compiler/graph.md#per-node-bumps)). This is the
    board's `dismiss` action ([board](#board)).
  - The compiler never rewrites a source document without an accepted proposal. The
    same semantics serve the [`edit_fact` chat tool](../compiler/tools.md#chat-tools).

Generation and verification:

- `GET /api/gen/pending`, `GET /api/gen/package/{entity}`: the open `generate` and
  `bind` goals as [generation](../consumers/gen.md) rows, and the per-entity
  generation package a session receives (`404` for an unknown entity).
- `GET /api/verify/pending`: pending verification grouped by reason.
- `GET /api/verify/matrix`: every ledger row with its
  [derived status](../consumers/gen.md#status-is-derived-never-stored), plus rollup
  counts. `?format=csv` or `?format=md` answers the same rows as a file for
  download ([trace matrix](#trace-matrix)).

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
  and the test artifacts pointing at it. The reply also carries the recorded `medium`
  and `build` from the ledger, the [product form](#product) page's source. Hidden entries, `target`, `node_modules`, and
  `jazyk-out*` directories are skipped, so a deliverable at the project root lists the
  product, not the compiler's output.
- `GET /api/deliverable/file?path=`: one file's text plus its resolved
  [sites](../consumers/gen.md#traceability): each ledger site whose `file` is this
  path, located against the current text (`exact`, `moved`, or `lost`), and each test
  whose artifact is this path, located by its embedded test name. Every entry names
  its requirement and current verification status. A file that is not text reports its
  size instead. Reads are confined to the deliverable directory: a path that leaves
  it is `400`, a missing file is `404`.
- `GET /api/deliverable/baseline?path=`: the file as it stood before the last
  generation run rewrote it, from the snapshot generation takes at write time (see
  [incremental regeneration](../consumers/gen.md#incremental-regeneration)). `404`
  when generation never rewrote the file. Reads are confined to the baseline
  directory.

The out directory:

- `GET /api/out/file?path=`: one file under the out directory, as its bytes with the
  content type its extension names (`image/svg+xml` for `.svg`, `image/png` for
  `.png`, `text/markdown` for `.md`, `text/plain` for `.puml`, `.yaml`, `.txt`,
  `.json`, and `.jsonl`, `application/octet-stream` otherwise). Read-only, no write
  verb. The path is relative to the out directory and validated: no absolute path,
  no traversal, no hidden component, no symlink escape; a path that leaves the out
  directory is refused with `400`, a missing file is `404`. This is what the
  [markdown preview](#markdown-preview) loads the generated pages from and what its
  inline images point at: an image element carries no bearer header, so the `token`
  query parameter authorizes it.
- `GET /api/out/list?path=`: the files under one directory of the out directory,
  recursively, each with its path (relative to the out directory) and size, hidden
  entries skipped. The same validation as the file read. The explorer lists
  `docsgen/` with it.

Diagnostics:

- `POST /api/diagnostics/{id}/triage` with `{triage}`: set the human
  [triage state](../compiler/model/diagnostic.md#lifecycle-and-triage)
  (`acknowledged`, `suppressed`, `wontfix`, or null to clear). The write commits
  through the store as a journaled changeset (`kind: triage`).
- `GET /api/questions`: the standing questions the [questions list](#questions)
  renders: every open, unsuppressed diagnostic carrying a
  [prompt](../compiler/model/diagnostic.md#prompts), each with its prompt and any
  recorded answer.
- `POST /api/questions/{id}/answer` with `{option}` (an option index) or `{text}`
  (a freeform reply): answer a prompted diagnostic, the same engine every frontend
  uses ([answers](../compiler/model/diagnostic.md#answers)). An `edit` option
  applies and resolves in the reply; any other answer records handling and an
  [answer session](./acp.md#answer-sessions) acts on it.

Transcripts:

- `GET /api/trace`: past runs from the transcripts on disk under `<out>/trace/`,
  newest first (by the start the metadata line records), capped at 200: each with
  its stem, its metadata line, its outcome when the run finished (a missing outcome
  means it died mid-run), and its event count. The [Runs](#runs) page's
  run list.
- `GET /api/trace/{stem}`: one whole transcript: the metadata, the outcome, and
  every event, elided the same way the live stream is ([jobs](#jobs)). Event `n`
  with nothing cut is `GET /api/trace/{stem}/{n}` ([jobs](#jobs)).

## Jobs

The GUI runs builds and workers itself. `POST /api/jobs` with
`{kind: compile | gen | verify | audit | decompile | benchmark}` (plus targets and `force` where
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
- The file holds the [full payloads](../compiler/sessions.md#trace-events): every
  prompt sent and every reply received. What travels to the browser is elided: a
  string over 2000 characters becomes a preview naming its full length, and every
  object holding one carries `elided: true`. `GET /api/trace/{stem}/{n}` returns event
  `n` of that transcript with nothing cut. Expanding a row in the activity panel is
  that fetch. A running job's events are readable the same way, by the same number,
  because the file is flushed per line.
- The metadata line and the outcome line each record the store generation at that
  moment, so a run's committed changesets are exactly the journal entries between
  the two. The activity panel renders them inline with the trace, and the
  whole-build report (`GET /api/ripple?generation=` rooted at the first) is one click
  from the run.
- Every build leaves a transcript, whichever frontend ran it: the CLI `compile`,
  `check`, `gen`, and `test` commands persist the same file (the metadata line carries
  `source: cli` and no job id), so the activity panel also lists builds that ran
  outside the GUI. See [CLI](./cli.md).
- Jobs run in-process, one at a time, in submission order. Compilation is sequential
  by design: one build under the build lease, one session at a time within it. Every
  kind contends on the store lock and the LLM budget, so serializing them is the
  point.
- Submitting a `compile` while one is already queued returns the queued job's id.
- Cancellation is best effort: a job stops at its next boundary (between sessions,
  entities, or rows). An LLM call already in flight is not interrupted. A cancelled
  compile parks its remaining goals
  ([parked and failed](../compiler/reconciler.md#parked-and-failed)), and the next
  build resumes them first.
- Job progress streams as [trace events](../compiler/sessions.md#trace-events) over
  the event stream, the same events the `compile` command renders as its live trace:
  `batchStart`, `sessionStart`, `sessionDone`, `sessionFailed`, `gcBurst`, the `goal`
  events with cause and justification, and the tool rows.

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
- `board.changed`: the board was re-derived, after a commit, a document save, a
  control change, or a triage. Carries the counts by class, kind, and state; the client
  refetches `GET /api/board` for the cards.
- `goal.opened`: one goal joined the board. Carries the goal, its cause (generation,
  mutation, `via`), and the batch that opened it when a session did. One event per
  entry in a committed changeset's `opened_goals`.
- `goal.resolved`: one goal left the board resolved. Carries the goal, the generation,
  and the justification. One event per entry in `resolved_goals`. Failures and
  parkings are state changes on the card and travel as `board.changed`.
- `docs.changed`: matched documents changed on disk, with whether the graph is stale
  against them.
- `pending.changed`: the verification worklist or the
  [unclaimed report](../consumers/bind.md#the-unclaimed-report) changed size. Bind and
  generate work sits on the board, so its counts travel as `board.changed`.
- `watch.state`: the workflow modes changed (compile or generation). Carries both.
- `control.changed`: the [control plane](../compiler/control-plane.md)
  moved: a release landed, a worker registered or dropped, a lease was taken or
  freed. Carries the workers snapshot the workers strip renders.

External activity surfaces the same way: a `jazyk compile` run from a terminal, or an
[MCP](./mcp.md) agent committing through write tools, moves the lock and the counter,
and the GUI renders it live without owning the job. The goal events come from the
journal entries at commit, not from the job, so a build run anywhere moves the board.

## Product surface

The GUI is the product's front door: a hosted or local editor for the people who own
the documents and the product they describe. The compiler's machinery (goals, cones,
tiers, batches, leases, releases, sessions, GC, the verdict) stays behind one
[Developer toggle](#developer-toggle). The default mode speaks the product words in
the [vocabulary](#vocabulary) table, organizes the workspace around three
[lenses](#lenses) over one graph, and gathers every request the compiler makes of a
human into one [Inbox](#inbox).

Who it serves:

- An author (a product manager, an architect, a standards engineer, a technical writer,
  a novelist, an operations lead) writes and reviews the documents. They want to know
  whether the prose was understood, where it is ambiguous or contradictory, what was
  extracted from a sentence, which questions they owe, and how an entity connects to
  the rest.
- An implementer (an engineer, or an agent the engineer supervises) owns the product.
  They want which requirements are verified, failing, unimplemented, or stale; the
  trail from a sentence to a criterion to a test to a code site; and what the product
  carries that no document describes.
- A reviewer reads: the documents with their diagrams and linkage, a health summary,
  what changed between two syncs, and a chat to ask the project questions.
- A jazyk developer wants the workbench: the board, the runs, the prompts, the costs.
  The Developer toggle serves them without shaping the default layout.

Three project shapes are served equally:

- Documents and product fully linked: the trail runs both ways and the
  [trace matrix](#trace-matrix) is the health dashboard.
- Documents covering a slice of the product (a standard's requirements, a document for
  one feature): the documents are authoritative for their slice only. How much of the
  product the documents describe is a number, never a failure. The files no document
  describes ([not described](#not-described)) are informational and the entry point
  to documenting more. For a standard, the trace matrix reads as a compliance matrix
  and exports.
- Documents written out of the product ([decompilation](../consumers/decompile.md)):
  product first, drafts land as ordinary documents carrying `unratified`, the review
  surface is the editor, and progress is a count of areas documented.

A non-software deliverable (a novel, a handbook, a slide deck) is a product too: the
default mode says "product", never "code", unless the recorded
[medium](../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated) is code.

## Vocabulary

The default mode never says goal, cone, tier, batch, lease, release, session, GC, or
verdict. One table maps the compiler's words to the product's; the app, the CLI help
where the two meet, and this page use it. Developer mode shows the compiler word
beside the product word wherever both apply.

| Compiler (docs, CLI, Developer mode) | Default mode |
|---|---|
| compile, build | Sync: the model syncs with the documents |
| bind, generate | Build: the product is built from the documents |
| verify, audit | Check: tests run, statuses refresh |
| decompile | Document: documents are written from the product |
| release (manual mode) | the Sync or Build button itself; gated work is Pending changes |
| `compile: auto`, `compile: manual` | Sync on save, Sync on request |
| `gen: auto`, `gen: manual` | Build after sync, Build on request |
| verdict `converged` | Up to date |
| verdict `incomplete` with `open` and `failed` | Sync incomplete, N items left, with the reason |
| `blocked` (`ratify`, `answer`, gated) | Waiting on you (N): the Inbox count |
| `optional` (soft-limit GC advice) | Suggestions |
| a goal in session, batch narration | the build thread's plain lines ("Reading compiler/graph.md, 3 sections") |
| diagnostic, by severity | Problem: error, warning, note |
| prompted diagnostic | Question |
| ratification proposal, invented choice | Proposal |
| triage `acknowledged`, `suppressed`, `wontfix` | Seen, Hidden, Won't fix |
| coverage `covered`, `non-normative`, `unprocessed` | Read, Informational (with its note), Not read yet |
| provenance `quote` | Quoted, with the sentence |
| provenance `derived` | Inferred, needs your confirmation (the Proposal) |
| provenance `decree` | Your edit, not yet in the documents (the Proposal) |
| entity, requirement, criterion | kept: these are the product's value |
| view | Diagram |
| relationship, state machine | Relation, States |
| deliverable, medium | Product, Product form |
| ledger row | Trace row |
| status `verified`, `failing`, `partial` | Passing, Failing, Partly passing |
| status `missing`, `unimplemented` | No test yet, Not built yet |
| status `stale-requirement`, `stale-test`, `stale-code` | Outdated (the requirement changed), Needs a rerun |
| relationship type, from the entity's side | a verb: uses, is used by, is part of, is made of, has, belongs to, is a kind of, implements, is related to |
| stereotype | the kind in words beside the name (a component, an actor), never in guillemets |
| section id (`#/backend/database`) | the document's file name and the section's title (backend.md › Database) |
| criterion kind `positive`, `negative`, `boundary`, `quality` | Example, Failure case, Edge case, Measure |
| unclaimed report | Not described: files no document describes |
| draft (`unratified`) | Draft |
| journal, generation, changeset | History, Sync #N, Change |
| ripple | What this caused |

### Presentation

A page reads without knowing jazyk. Four rules hold on every page and in the
inspector:

- Names and sentences, never ids. An entity shows as its name, a requirement as its
  statement, a criterion as its statement, a document reference as the file name and
  the section's title. An id appears only in Developer mode and in a tooltip.
- Each fact once. A page never states a fact in two places: an entity's relations are
  its connections picture, its children are the picture's `contains` arrows, its
  requirements are one list. A second rendering of the same fact is a Developer
  detail or nothing.
- The compiler's own records are Developer details: the history of the goals that
  touched a node, what a change caused, the loaded context, the diagram membership
  records, the relation ids, the provenance's reasoning. They sit at the foot of the
  page under `Developer details`, shown only in Developer mode, so nothing is lost.
- Long lists start short. Past a handful of rows a list shows the first few and a
  `show all N` toggle.

## Backends and capabilities

One app, three backends. The [server](#serving) (`jazyk gui`) serves everything; the
demo runs a bundled example in the tab; the [browser backend](./wasm.md) runs the
compiler in the tab over a folder on the computer. The backends do not change the
layout. They change what the backend can do, so the app reads one descriptor and
renders against it:

- `GET /api/capabilities`:
  - `projects`: `list` (the server's recent projects and a path input), `bundled`
    (the demo's examples), `folder-picker` (the browser backend's File System Access
    flow).
  - `docs`: `read` or `write`.
  - `events`: `stream` (the [event stream](#events)), `pushed` (trace events posted
    from a build running in the page), `refetch` (no stream; the app refetches after
    every write).
  - `lsp`: whether `GET /lsp` exists. Without it the editor derives the same
    decorations from the graph's mentions and quotes on the client.
  - `jobs`: the job kinds the backend runs, from `compile`, `gen`, `verify`, `audit`,
    `decompile`, `benchmark`.
  - `agent`: `embedded`, `external`, or `none`. Without an agent the chat column
    shows build threads only and says why.
  - `model`: `configured`, `in-page` (a model loaded in the tab), `loadable` (the
    tab can load one, on request), `needs-key`, or `none`.
  - `rendering`: `server` or `none`. A tab renders no `.svg`; a demo seed ships its
    renderings.
  - `persistence`: `disk`, `memory` (the demo: a reload starts from the seed), or
    `browser` (the folder handle, mirrored back).
- A route the backend does not serve answers `501` with a reason
  (the browser backend's rule). A panel whose read answers `501` hides
  behind that reason; it never renders empty or errors.
- A missing capability hides its action and explains it in the empty state ("Sync
  needs a model. Add one in Settings."). Nothing fails for want of a capability.
- The transport is one seam: every read and write goes through one
  `request(method, path, body)` the backend implements, so the app never knows which
  backend it has beyond the descriptor.

The demo banners "Demo: changes stay in this tab". The browser backend banners the
folder it mirrors to.

## Home

The first screen is the project picker:

- Open a project: the entries the descriptor allows. `Open folder` (the browser
  backend, with a `read-only` choice beside it, see
  [a folder on disk](./wasm.md#a-folder-on-disk)), `Try a demo` (the bundled
  examples: the shop, the sort utility, the ERP, the slide deck, the novel, the
  organization), `Recent` (the server's list from `GET /api/projects`: each with its
  path, its shape, its sync state in product words, and its waiting count).
- In the browser the `Recent` list is the browser's own, kept in its local storage
  under `jazyk.recent`: every demo and folder opened in this browser, newest first,
  each with its name, its kind, and a folder's mode. A folder's directory handle
  cannot be written as text, so it is kept in the browser's IndexedDB under the
  entry's id. Clicking an entry reopens it; a folder asks the browser for permission
  again, since the grant does not outlive the tab. An entry can be removed from the
  list. The `read-only` choice is remembered the same way.
- New project: a wizard. Pick the documents folder and glob, optionally the product
  folder, pick the model or agent (or "none: browse only"), name the root documents.
  The wizard writes `jazyk.toml` through `PUT /api/settings`, then runs the first sync
  with its plan visible and an estimate beside it. It names the shape it detected:
  "documents only", "documents and product", or "product only: start by documenting
  it", the last offering Document.
- The Developer toggle sits here too, so a developer lands in the workbench from the
  first project.

`GET /api/projects` lists the projects the server has opened, newest first, each with
its root, its shape, and its status summary; the server keeps the list in
`~/.jazyk/projects.yaml`.

## Layout

One workspace page. Navigation swaps panes, never the page. Five regions:

- The top bar: the project switcher, the three lenses as tabs (Docs, Model, Product),
  Inbox with its count, History, the command palette, the action button, and settings.
- The navigator: the left column, the active lens's navigation.
- The center: the open item. The editor, a model page, a product page, the Inbox, a
  History diff, the settings form.
- The inspector: the detail pane beside the center, never replacing it. Selecting a
  node anywhere (a sentence marker, a diagram box, a list row, an id chip, a code
  lens) shows its detail here. Closable (its close button, or Escape outside a text
  field); the center keeps its state under it. On a narrow screen it is a drawer.
- The chat column: the far right, collapsible to a strip with an unread mark. See
  [chat](#chat).

The status bar under everything: the sync number, the state in product words, the
waiting count, the problem counts by severity, the connection state, and the
[Developer toggle](#developer-toggle). The state never travels alone: the problem
counts ride beside it, because a project can be up to date with open errors standing
([convergence](../compiler/compilation.md#convergence)).

There is no bottom activity panel. Its control line is the action button, its build
progress is the chat column's build thread, and its run console is the Developer
lens's [Runs](#runs).

On a narrow screen (a phone, 760 pixels wide or less) the center takes the width.
The navigator is a drawer opened from a menu button at the left of the top bar and
closed by any navigation, the inspector is a drawer over the center shown only while
a node is selected, the chat column is hidden, the editor's preview stacks under the
text, and the top bar and the status bar scroll sideways.

Until a project is open, the top bar shows only the project switcher and every route
but Home redirects to Home. A server always has one open; the browser backend has
none until a demo or a folder is picked, and none again after a reload.

Addressable state: `/docs/<path>` (a document in the editor), `/model/<section>` and
`/model/<section>/<id>` (entities, requirements, criteria, diagrams, relations,
states), `/product/<section>` and `/product/files/<path>` (files, trace, tests,
not-described, generated, form), `/inbox/<section>`, `/history/<generation>`,
`/settings`, and `/dev/<section>` under the Developer lens. `?node=` holds the
inspector selection; a document takes `?section=` and `?quote=` to reveal and
highlight a quote; a product file takes `?site=<requirement>` or `?line=`; a diagram
takes `?detail=` ([explore](#explore)); `?run=` selects a run under Developer. Every
URL the previous layout served (`/files/*`, `/graph`, `/board`, `/work`, `/journal/*`,
`/feedback`, `/benchmarks`) redirects to its new place.

### The action button

One button in the top bar whose label is the state: `Sync (3 documents changed)`,
`Syncing 2 of 5`, `Up to date`, `Build (4 entities)`, `Check`. Clicking it opens the
action sheet:

- What will run, in product words: the documents, the work counts, who runs it (the
  built-in runner or the attached agent, by the
  [dispatch](../compiler/control-plane.md#dispatch) preference), and an estimate from
  the last sync's cost (`status.yaml` `costs`).
- The mode switches: Sync on save or on request; Build after sync or on request. The
  modes are the [workflow modes](#workflow-modes): control-plane state, never the
  app's.
- `Run`. In on-request mode `Run` records the [release](../compiler/control-plane.md#modes-and-releases)
  and starts the job, or wakes the attached agent. When the preference is an agent
  and none is registered, the sheet says so and offers the built-in run.
- Check and Document are entries in the same sheet. Document takes an area from the
  [not described](#not-described) list and records a decompile release.
- While a job runs the button shows its progress and offers cancel.

In Developer mode the sheet also opens the [preview pane](#preview) on the next batch.

## Lenses

Three navigators over one graph. Selecting anything shows the same card in the
inspector and highlights its linkage in every visible lens: a document tints the
product files its requirements bind to and the entities it defines; a product file
tints the documents its requirements anchor in; an entity tints its mentions.

### Docs

The navigator is the document tree. Each row carries its problem badge (the highest
open severity, colored), the drift dot when it is stale against the model, the pending
mark when it was edited since its last sync, and the progress mark while a sync works
on it (dimmed and waiting, then running with the section reached, then its result,
fading after a few seconds and held under the pointer). Documents are created, moved,
and deleted from the tree through the documents API; a delete asks for a second click,
never a dialog.

Under the tree, the open document's outline: its sections from the
[section tree](../compiler/parsing.md) in document order, the top-level ones included
(the document's `# heading`, or each of them when it has several) with their
subsections indented under them, each with its read state (Read, Informational with
its note, Not read yet). Clicking a section scrolls the editor to it.

The center is the [editor](#editor). A Reader toggle renders the document with the
same overlays and no editing, for reviewers.

The inspector for a section: its read state, the requirements extracted from it with
their statements, the entities it defines, its problems, and what the last sync did
here.

### Model

The navigator has six sections:

- Entities: the containment tree, one root per scope
  ([levels](../compiler/concepts/levels.md#levels)), each entity with its stereotype
  chip and requirement count. A grouping (`derived` provenance) is marked, its
  proposal one click away.
- Requirements: a list filterable by document, entity, provenance, and trace status,
  searchable by statement.
- Criteria: the [criteria](../compiler/model/criterion.md) per requirement with kind
  and binding status.
- Diagrams: every [view](../compiler/model/view.md) grouped by kind, defaults marked,
  each with its member count and limits state.
- Relations: the derived relationships, grouped by type.
- States: the derived state machines, one per subject.

The center is a page per selection:

- The entity page, top to bottom, each fact once ([presentation](#presentation)):
  - The header: the name, the kind in words, the path of the entities it is part of
    (each a link), the definition (editable), its other names, and its attributes.
    A grouping no document states says so in a line with its proposal.
  - One status line: how many requirements it carries and how many pass, and the
    files that implement it, or "Not built yet".
  - Connections: one picture of the entity and its one-hop neighbors, each arrow
    labeled with its relation in words, the parent as `is part of` and the children as
    `contains`. Selecting a neighbor lists the sentences that connect the two, each
    opening its requirement, and a link to the neighbor's page. There is no second
    list of the same relations.
  - Diagrams: the diagrams that show its inside or its flows, by title.
  - Requirements: each statement with its status in words, opening its page.
  - In the documents: each mention as the file name, the section title, and the
    quoted sentence, opening the editor at the quote.
  - Problems: the open problems naming it.
  - Developer details, in Developer mode only.
- The requirement page, top to bottom:
  - The statement (editable), and the entities it is about, by name.
  - One status line in words: the status, the test's name, the implementing files.
  - Where it comes from: the file, the section title, and the quoted sentence, opening
    the editor at the quote; or, when no document states it, `Inferred` or `Your
    edit` with the reasoning and the proposal's Accept and Retract.
  - Acceptance criteria: each criterion's statement, its kind in words, its measure,
    and where the criterion comes from (the quote, or `Inferred`). The statement
    carries the condition; a criterion split into given, when, and then shows the
    split on hover, not as a second sentence.
  - Details, only when present: the relations it states in words ("Backend uses
    Database"), the state change it declares ("Order: placed → paid, on payment"),
    and its code sites (`exact`, `moved`, `lost`).
  - Developer details, in Developer mode only: the id, the facets with their
    reasoning, the evidence, the history, and what it caused.
- The diagram page: the rendered diagram with live drill-down anchors
  ([drill-down](../compiler/diagrams.md#drill-down)), then the members by name. The
  excluded members with notes, the query, the collapse set, the limits state, and
  membership edits (a decree, which clears `default` with the warning) are Developer
  details. The Diagrams section lands on a gallery by kind, each diagram by title
  with its member count.
- The criteria page: the given, when, and then of each criterion, its kind, its
  measure, and its binding.

Every id in a page opens the inspector; the page never changes under it.

#### Explore

The walk the [cards](../consumers/docsgen.md#entity-cards) give a markdown reader,
served live from `GET /api/entities/{id}/card` and `GET /api/views/{id}/page`:

- Every entity is a page first, laid out as the [entity page](#model) above: the
  path of what it is part of, its connections, its diagrams, each item a link that
  moves the walk. A diagram's boxes are live: clicking one opens that entity and,
  when it has a level, offers the level below. The card's `Sits in`, `Inside`, and
  `Siblings` rows are the [docsgen card](../consumers/docsgen.md#entity-cards)'s
  vocabulary for a markdown reader; the GUI says the same as the path, the Diagrams
  list, and the parent's page.
- History: the walk keeps a stack of positions (`/model/entities/<id>` with `?view=`).
  Back and forward walk it, and the browser's back lands on the same entry.
- Detail: a level diagram opens with its groupings collapsed; `more detail` expands
  every grouping one level, `less detail` collapses one back. The diagram redraws
  through `GET /api/views/{id}?detail=`; nothing is rendered by the build for it.
  `?detail=` holds it.
- Sideways: the card's siblings and the page's `Around` list are chips: the level
  above, the other diagrams of the same level, the levels below.
- The neighborhood widget: the Connections picture, the selected entity and its
  one-hop neighbors with each arrow's relation in words, drawn from the graph. It is
  the only map the default mode shows; the whole-graph map is a Developer tool
  ([map](#map)).

### Product

The navigator has six sections:

- Files: the product tree, each file with its ownership badge (the entities and
  requirements the [ledger](../consumers/gen.md#the-ledger) binds to it) and a stale
  dot after a build changed it.
- Trace: the [trace matrix](#trace-matrix).
- Tests: the test artifacts by requirement, with kind, run command, verdict, and
  evidence.
- Not described: the files no document describes ([not described](#not-described)).
- Generated docs: the pages [docsgen](../consumers/docsgen.md) writes, rendered.
- Product form: the recorded [medium](../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated): form,
  toolchain, build command, last run, and the medium warning when one stands.

#### Product file viewer

A product file opens read-only in the center:

- Every resolved [site](../consumers/gen.md#traceability) shows as a code lens above
  its line (the requirement id and trace status). Clicking it opens the requirement
  in the inspector and highlights the sentence in the Docs lens. Lost sites are
  flagged.
- The viewer diffs against the file's build baseline
  (`GET /api/deliverable/baseline`): gutter marks on the lines the last build changed,
  and a `diff` toggle for a side-by-side view. A file with no baseline shows neither.
- A markdown product file opens on its [markdown preview](#markdown-preview) with a
  `source` toggle.

#### Trace matrix

Every ledger row, the [verification matrix](../consumers/gen.md#status-is-derived-never-stored)
served by `GET /api/verify/matrix`:

- Rows are requirements grouped by entity and document. Columns are the criteria with
  their test verdicts. The row carries its status chip, its reason, and the
  one-sentence repair; the [cascade](../consumers/gen.md#the-cascade) is explained per
  row.
- Filters by status, document, and entity. Actions per row and per entity: Build,
  Check, re-bind.
- The header: verified, failing, stale, unverified, not built; and the two coverage
  numbers: sections read (the graph's coverage of the documents) and product files
  described (the ledger's coverage of the product).
- A requirement a document marks as not applicable shows as such, never as missing.
- `GET /api/verify/matrix?format=csv` and `?format=md` export the matrix, the
  compliance-matrix use.

#### Not described

The [unclaimed report](../consumers/bind.md#the-unclaimed-report) as a file list with
sizes and the unattached ratio per entity. An area picker groups files by directory or
module. The Document action records a decompile release for the picked area and
dispatches ([decompilation](../consumers/decompile.md#triggering)); the drafts it
produces appear in the [Inbox](#drafts). The count shrinks as drafts land and their
statements bind: stopping early documents a subsystem, and the number says how far.

#### Markdown preview

A markdown file opens rendered: headings, lists, links, inline and fenced code,
tables, block quotes, and images, in the app's palette. Which surface holds the
preview depends on the tree the file lives in:

- A generated page under the out directory and a markdown product file open on the
  preview in place of the source; a `source` toggle swaps the read-only text in. Any
  other file under the out directory opens as read-only text, a `.png` as the image,
  and a `.svg` inline with its links live.
- A source document keeps its [editor](#editor) and shows the preview beside it,
  split, live as one types; a `preview` toggle on the editor's bar shows or hides it,
  remembered in the browser.

Links and images resolve against the open file's path, so the pages walk the way
[docsgen](../consumers/docsgen.md#entity-cards) writes them: a relative link to a
page under the out directory opens that page, one to a document opens the editor, one
to a product file opens the viewer, a `#fragment` scrolls to the heading with that
slug under the compiler's own slug rule, a link with a scheme opens in a new tab, and
a link that lands nowhere renders as text. A relative image under the out directory
renders inline through `GET /api/out/file` with the `token` query parameter. A `.svg`
renders inline as SVG, sanitized (`<script>` elements and `on*` attributes dropped),
its `viewBox` kept and its hard `width` and `height` dropped, so the anchors the
renderer wrote ([drill-down](../compiler/diagrams.md#drill-down)) are live.

The preview renders the text as it stands and never rewrites it.

## Inbox

The single review queue: every request the compiler makes of a human, in one place,
with one vocabulary. The Inbox count in the top bar is the sum of Questions,
Proposals, Pending changes, and Problems at `error`. `GET /api/inbox` serves the
rollup with per-section counts; each section reads its own endpoint.

### Questions

Open diagnostics carrying a [prompt](../compiler/model/diagnostic.md#prompts), from
`GET /api/questions`. Each item: the question, the sentence it concerns with a jump to
the editor, up to four option buttons, and a text box when the prompt accepts
freeform. Answering posts to `POST /api/questions/{id}/answer`, the same engine every
frontend uses: an `edit` option applies and the item disappears with the next event;
other answers show `answer.status` as the [answer session](./acp.md#answer-sessions)
handles them (`handling`, `handled`, `failed`).

The same questions render inline in the [editor](#editor) as a band under the
anchored sentence, with the same buttons. The chat keeps `/questions` for agents.

Two goal kinds wait on this list, and Developer mode's board says so on their cards:
an [`answer`](../compiler/goals/answer.md) goal waits on an unanswered prompt, and a
[`ratify`](../compiler/goals/ratify.md) goal waits on a proposal. Opening a project
with standing questions re-surfaces them here without any action: the list reads from
the graph, and the graph kept them.

### Proposals

[Ratification proposals](../compiler/model/diagnostic.md#ratification-proposals)
grouped by target document, one reviewed draft per document as the
[ratification report](../consumers/docsgen.md#ratification-proposals) groups them.
Each: the sentence the document should gain, the target section, `Accept` (the dual
write: the sentence lands, the provenance flips to `quote`, the diagnostic resolves,
the goal leaves), and `Retract` (`retract_decree`). The owner may also write the
sentence anywhere in their own words; the proposal is a convenience, never the only
door. Invented-choice prompts from a build ([invented choices](../consumers/gen.md#invented-choices))
list here too, graded by their severity.

### Problems

Open diagnostics by severity: the rule, the message, the reasoning, the subjects
(each a jump: a node opens the inspector, a section opens the editor), and the triage
actions (Seen, Hidden, Won't fix, posted to `POST /api/diagnostics/{id}/triage`).
Hidden items sit behind a filter and never count. A `failing` trace row is a problem
here ([failing bindings](../consumers/bind.md#what-the-verdict-means)). The filters:
severity, rule, document.

### Pending changes

In on-request mode, the documents edited since their last release
([gating](../compiler/control-plane.md#modes-and-releases)): each with what a sync
would do to it, in product words, and the Sync action that records the release. The
Build release lists the same way: the entities a build would produce. This is the
product face of `awaiting release`: visible here, actionable here, and nowhere else
described as blocked.

### Drafts

Documents the Document action wrote that still carry `unratified`
([ratification](../consumers/decompile.md#ratification)): each with its evidence
summary (how many statements observed by a test, how many inferred from the code) and
`Open in editor`. Editing the document, even to accept it with a one-line change,
clears the item.

## History

For reviewers and release notes:

- The list of syncs, newest first, from `GET /api/journal`: each with its state in
  product words, its cost in plain words (sessions, tokens), and the human edits that
  started its cascade (`edit`, `dual-write`, `decree`, `ratify`, `answer` entries),
  so the list separates what people decided from what followed
  ([release diffs](../consumers/pm.md#release-diffs-from-the-journal)).
- A sync opens its changes: each mutation with its reasoning, the work it resolved
  with the justification, the work it opened.
- A range of syncs opens the diff (`GET /api/diff?from=&to=`): per node before and
  after.
- What this caused: one change's [ripple](../compiler/reconciler.md) rendered as a
  small DAG (`GET /api/ripple`), forward or back.

## Chat

The chat column is the GUI's [ACP client](./acp.md) surface: persistent,
project-scoped, collapsible to a strip with an unread mark. One list, two kinds of
thread:

- Chat sessions: a conversation with the configured agent, created in the column. The
  session gets the [`chat` serving](./mcp.md#toolsets), so the agent can read and load
  the graph, revise requirements and edit facts through the
  [dual-write tools](./acp.md#dual-write-tools), claim goal batches through the
  lifecycle tools, and edit project settings. Prompting streams the agent's thoughts,
  messages, and tool calls into the transcript as they happen. The title is the first
  prompt line.
- Build threads: one per job, live while it runs, kept after. Every
  [worker session](./acp.md#worker-sessions) a job runs registers as a read-only
  session, so watching a sync is opening its thread. A build thread renders the
  [plan](./acp.md#plans) as a live checklist, one entry per goal batch (pending,
  running, done, failed, or waiting with the reason), the running batch's narration
  as short lines in product words, the committed changes as collapsible cards, and
  the state line at the end. In Developer mode a board card opens the thread holding
  its goal.

The column's behaviors:

- Ask about this: the current selection (the open document, the inspected node) is
  offered as context with one click, so a question about a requirement starts with the
  requirement loaded.
- Tool calls collapse to one line each in the default mode. The
  [Developer toggle](#developer-toggle) expands their payloads, the rounds, the
  tokens, and the loaded-set panel: the [loaded set](../compiler/context.md#the-loaded-set)
  as the serving renders it, re-rendered on every mutating tool reply, each item
  opening in the inspector.
- [Slash commands](./acp.md#slash-commands): the same catalog the IDE proxy
  advertises, completed in the prompt box. A build command runs through the job
  queue and streams into a build thread.
- Follow in editor: a toggle on a build thread that pins the transcript to the newest
  update and moves the center along with the work. A tool call carrying a location
  opens the document or product file at that line.
- A dual write the agent makes shows as a diff card with a jump to the sentence.
- Permission requests from chat sessions surface inline as option buttons
  ([permissions](./acp.md#permissions)). An unanswered request cancels with the
  prompt it belongs to. Worker sessions never ask; their policy answers.
- Transcripts persist in the [session store](./acp.md#session-store) under
  `<out>/sessions/`, so a reloaded page and a restarted server restore the list and
  its history. A restored conversation has no agent behind it until it is prompted
  again.
- A backend without an agent shows build threads only and says why the chat is
  unavailable.

API: `POST /api/chat/sessions` creates a session, `GET /api/chat/sessions` lists
them, `GET /api/chat/sessions/{id}` returns one with its transcript and its loaded
set, `POST /api/chat/sessions/{id}/prompt` sends a prompt (progress streams over the
event stream), `POST /api/chat/sessions/{id}/cancel` cancels the open prompt, and
`POST /api/chat/permissions` answers a pending permission request with body
`{sessionId, id, optionId?}` (a missing `optionId` cancels the request). Updates
travel as `chat.update` events, elided like `job.trace`; permission requests as
`chat.permission`; session list changes as `chat.sessions`.

## Inspector

The detail pane for one node, opened from anywhere, layered over nothing. It shows
the same layout as the node's page ([entity page, requirement page](#model)), narrower,
under the same [presentation](#presentation) rules. What each kind carries:

- An entity: name, definition, scope, stereotype, parent (opens it) and children,
  attributes (each with its provenance), mentions (each opens the editor at the
  quote), the requirements referencing it, its relations by direction and type, the
  diagrams it belongs to, its derived states, the files implementing it, and its
  verification rollup. The [neighborhood widget](#explore) sits beside the trail.
- A requirement: the statement, its provenance (Quoted opens the editor at the quote;
  Inferred lists the upstream nodes and the reasoning; Your edit names the author and
  the note; both carry the proposal), its entities, its edges with type and
  cardinality, its transition, its facets with reasoning, its criteria with their
  tests and verdicts, its sites (each opens the product file at the located line),
  and its trace status.
- A criterion: its requirement, the condition and outcome, kind, measure, its test,
  the last verdict and evidence.
- A diagram: kind, title, the members in order, the excluded members with notes, the
  query, the collapse set, provenance, limits state, and the open action.
- A relation: the members and the contribution groups, each with its requirements. A
  lifted arrow lists the concrete edges beneath it first.
- States: the states, the initial state, the transitions, each with the requirement
  that declares it and its open [checks](../compiler/model/state-machine.md#checks).
- A problem: message, severity, subjects, reasoning, the triage actions, and its
  question when it carries one.
- A product file: the entities and requirements bound to it, its sites, its tests.
- A document, when nothing is selected in it: the requirements its sentences state,
  by statement with their status, and the files that implement them.
- In Developer mode: a goal (kind, class, target, change, cause, state, hints, and its
  explanation from `GET /api/explain`), a change record, and a `Raw` tab with the
  node's YAML.

Justification walks. Every rendered element walks to the sentence behind it, and the
inspector is the walk ([justification closure](../compiler/compilation.md#checks)):
an arrow opens its relation, the relation lists its contributing requirements, each
requirement shows its sentence, and the sentence opens the editor at the quote. A
walk that reaches an Inferred or Your edit fact ends on its open proposal instead of
a quote, with the upstream nodes beside it.

Editing facts. Fields in the inspector are editable in place: a definition, an
attribute value, a statement, an edge's type or cardinality, a transition, a
diagram's members. Saving goes through `POST /api/facts/{id}/edit`, the
[edit paths](../compiler/compilation.md#edit-paths):

- A Quoted fact: the inspector shows the proposed sentence rewrite as a diff of the
  sentence in its document. Accepting commits the dual write (the graph mutation and
  the prose replacement in one changeset, the document not re-dirtied); the open
  editor on that document updates in place. Declining lands the edit as Your edit
  with a proposal.
- An Inferred or Your edit fact, or a fact added with no prose behind it: a decree.
  The proposal appears in the [Inbox](#proposals) at once.
- A default diagram: any edit clears its `default` mark and the recompute leaves it
  alone from that commit on ([default views](../compiler/model/view.md#default-views));
  the inspector says so before the save and offers no delete while `default` is set.

## Editor

The GUI embeds a code editor on the project's documents, backed by the language
server over `GET /lsp` (WebSocket, one JSON-RPC message per text frame, no
Content-Length framing) where the backend serves it. Each connection is its own
session with its own open-document overlay. The [capabilities](./lsp.md#capabilities)
are the LSP's: anchored diagnostics, hover with the most relevant diagram and the
entity's page link, the requirement card, go to definition, references, completion,
document links, code lens. Without an LSP the same decorations derive on the client
from the graph's mentions, quotes, and section tree.

- Markdown renders inline while editing. Headings take their size, emphasis and
  inline code take their style, links show their text, list bullets and quote bars
  draw as marks, and fenced code highlights in its own language. The markup syntax
  appears only where the selection touches it, so the document reads like a page and
  edits like text. The text itself stays plain markdown, byte for byte: the docs are
  compiler input, provenance quotes locate against the exact characters, and nothing
  is rewritten by rendering.
- Requirement sentences carry a marker in the left margin colored by their rollup:
  extracted only, verified, failing, stale, question open. Clicking the marker opens
  the requirement in the inspector. The marker replaces the code lens as the default
  rendering; the lens returns under the Developer toggle.
- Entity mentions are underlined so what is clickable is discoverable; hover shows
  the card, click opens the inspector.
- Hovering a requirement's sentence shows the [requirement card](./lsp.md#capabilities):
  the requirement, the code, and the test, each linked, with the trace status. The
  card's links stay inside the app.
- The linkage rail: a toggle that shows, beside each section, chips for the entities
  it defines, its requirement count, its criteria count, and its verification rollup.
- Questions and proposals render inline as a collapsible band under the sentence they
  concern, with the option buttons and the freeform input, the same calls as the
  [Inbox](#inbox).
- The read state renders beside the text from the section tree: Read, Informational,
  and Not read yet sections are visually distinct.
- A sync in progress is visible in the text. When a session takes this document's
  sections, the sections in its batch are banded as queued, and the section the
  session reached ([`section` events](../compiler/sessions.md#trace-events)) is banded
  as running. When the session ends, the bands become its result, green for committed
  and red for parked or failed, and clear a few seconds later; hovering a band holds
  it.
- A dual write shows in the text as it lands: the accepted rewrite replaces the
  sentence in place, the document does not re-dirty, and the gutter shows no change
  against the baseline for it.
- The editor diffs against the last-synced text (`GET /api/docs/baseline`): changed,
  added, and deleted lines mark the gutter, updated live. A `diff` toggle swaps the
  editor for a side-by-side diff; the current side stays editable. A never-synced
  document shows neither.
- The [markdown preview](#markdown-preview) sits beside the editor when its
  `preview` toggle is on. The Reader toggle replaces the editor with the rendered
  page carrying the same markers, underlines, bands, and read states.
- Saving writes through the documents API with the conditional hash, so an edit made
  outside the GUI is never silently overwritten.
- In the browser, where a write lands without asking (the demo, and a folder in
  writing mode), the editor saves on its own a beat after typing stops and when the
  document is left, so there is nothing to click. In a folder's read-only mode a
  save keeps the text in the tab and asks, in a bar over the editor, whether to write
  to disk now ([modes](./wasm.md#a-folder-on-disk)). When a build commits, the server
  republishes diagnostics for every open document, the same refresh the
  [LSP](./lsp.md#rebuilds-and-refresh) does.
- Document URIs are `file://` paths under the project root, as reported by
  `GET /api/project`.

## Settings

The [project settings](../compiler/project-settings.md) as a form over
`GET /api/settings` and `PUT /api/settings`, grouped: Documents (glob, roots,
handlers, lint rules), Model and agent (`[llm]`, `[acp]`, and the executors per goal
kind, each with the last sync's cost for that kind beside it), Product (`[gen]`:
folder, worker, code globs), Workflow (the mode defaults). Unset keys show their
effective defaults. The API key shows set or unset only. Saving is conditional on the
file hash and applies live. A Connection group shows the backend kind and its
[capabilities](#backends-and-capabilities).

## Workflow modes

The GUI always watches the documents (that is what `docs.changed` reports). What a
change triggers is the workflow mode, and the mode is not the GUI's private state: it
lives in the [control plane](../compiler/control-plane.md)
(`control.yaml` in the out directory), where the internal loop, `jazyk monitor`, and
every agent over MCP read the same policy. A mode set in the GUI survives a restart
and binds the agents too.

`GET /api/watch` and `PUT /api/watch` carry `{compile, gen}`, each `auto` or
`manual`; the action sheet shows them as Sync on save or on request and Build after
sync or on request:

- `compile: manual` (the default): changes queue visibly in the Inbox's
  [pending changes](#pending-changes). The board derives from the saved documents, so
  the work a save opens exists before any release, carrying `gated: true`. Syncing is
  an explicit `Run` in the action sheet, which records a
  [release](../compiler/control-plane.md#modes-and-releases), so an attached agent's
  watcher fires from the same click.
- `compile: auto`: changes sync automatically, the loop of
  [`jazyk watch`](./cli.md#jazyk-watch): debounced events, a fingerprint gate, backoff
  retries for `incomplete` builds.
- `gen: manual` (the default): Build runs on `Run`, which likewise records a release.
  The release covers [binding](../consumers/bind.md#when-binding-runs) too: open
  `bind` goals run before `generate` goals.
- `gen: auto`: a finished sync with open `generate` goals queues a `gen` job behind it.

Document has no mode: it is always an explicit action that records a decompile
release for its area ([decompilation](../consumers/decompile.md#triggering)).

`--watch` starts with `compile: auto`. Automatic modes spend LLM budget, so both are
opt-in. Running `jazyk watch` in a terminal beside the GUI is safe: builds serialize
on the build lease and commits on the store lock, and the second build derives an
empty board.

### Workers

`GET /api/workers` reports the control plane: the modes, the registered
[workers](../compiler/control-plane.md#workers-and-leases) with their heartbeats and
held batches, the live leases, and the gated goal counts. The action sheet names who
will run ("claude-code agent, waiting for you"), and the Developer lens's
[Workers](#workers-1) page renders the whole snapshot. `Run` dispatches by the
`worker` preference ([dispatch](../compiler/control-plane.md#dispatch)): with an agent
registered and preferred, the click records the release and the agent claims the
batches ([compilation over MCP](./mcp.md#compilation-over-mcp)), its progress
streaming into a build thread from the MCP transcript and onto the board from the
commits; otherwise the GUI runs its own job. `POST /api/release` with `{stage}`
records a release without running anything.

## Developer toggle

One switch in the status bar (also `⌘⇧D`), remembered per browser, off by default.
`jazyk gui --dev` starts with it on. When on:

- The Developer lens appears in the top bar with the pages below.
- Compiler words appear beside product words in labels and in the status bar.
- The inspector gains goal cards, change records, and the `Raw` tab.
- The chat expands tool payloads, rounds, tokens, and the loaded-set panel.
- The editor shows the requirement code lenses beside the markers.
- The status bar adds the event-stream sequence, the store lock state, and the last
  request's latency.
- Failed or dead runs surface under [Runs](#runs) only. The default mode shows a
  failed sync as one line with its reason and a `Details` link that flips the toggle.

### Board

The goal board: what compilation owes, why, and what it is doing about it. The board
is derived from disk on every consult
([goal derivation](../compiler/reconciler.md#goal-derivation)), so the GUI shows the
same board `jazyk status` counts, whichever process runs the build.

- The sidebar: the counts by class, kind, and state; the verdict line as the CLI prints
  it; filters by class, kind, state, and document; and this build's cost from
  `status.yaml` (`costs`: sessions, tokens, by kind and by class).
- The center: two columns, compile and GC
  ([compile and garbage collection](../compiler/compilation.md#compile-and-garbage-collection)).
  Compile cards group by [readiness tier](../compiler/reconciler.md#readiness), the
  ready tier first. GC cards carry their cone state: ready when no compile goal is open
  in the target's cone, otherwise waiting with the count of compile goals still open
  there. A running GC burst names itself in the column header, the `gcBurst` line.
- A card: the kind, the target with its unit (a document, a section, an entity, a
  pair, a view, a ledger row), `mandatory` or `optional` (and the hard threshold an
  optional card escalates at, [escalation](../compiler/reconciler.md#escalation)), the
  change in one line, the cause (generation, mutation, `via`; the generation opens the
  journal entry), the hints, and the state:
  - open: ready, or waiting with the readiness sentence;
  - in session: the batch id, the card pulsing while the session streams;
  - blocked: the reason: the unanswered prompt, the ratification proposal, or the
    gated release, each a link to where the human acts (the Inbox);
  - parked: out of budget, resumes first next build;
  - failed: the reason the session gave, standing on the target;
  - resolved: the card turns into its justification and stays until the build ends,
    then leaves the board; the journal keeps it.
- Cause lines: a card opened by a resolution draws a line from the card that resolved,
  while both show, and the line lights as the `goal.opened` event fires. The board is
  the ripple, live.
- A card click opens the build thread holding the goal in the chat column when a
  session holds it; otherwise it opens the goal in the inspector with its explanation
  (`GET /api/explain?target=`).
- Card actions: `preview` opens the [preview pane](#preview) on the batch this goal
  would join; `explain` and `ripple` open the inspector; a blocked `answer` card jumps
  to its question in the [Inbox](#questions), a blocked `ratify` card to its
  [proposal](#proposals); a `split-view` or `abstract-entity` card offers `dismiss`,
  which stages [`bump_limit`](../compiler/graph.md#mutations) on the node (through
  `POST /api/facts/{id}/edit` with `{field: limits.<limit>, value}`): a decree that
  raises the node's own threshold ([per-node bumps](../compiler/graph.md#per-node-bumps)).
  Dismissal is a graph write, never goal state, and the goal stops deriving until the
  raised threshold is crossed.
- The board never shows decompilation: drafts stay outside the goal board.

E.g.:

```
abstract-entity   ent:order                      GC · optional (mandatory at 80)
54 requirements > 50 (threshold-crossed)
cause: g412 #3 via requirements-per-entity
waiting: 2 compile goals open in the cone
hints: load ent:order · skill abstraction
```

### Runs

The run console: a run is one job plus what it committed.

- The run list: newest first, live jobs and the transcripts on disk (CLI and MCP runs
  included, from `GET /api/trace`), each with kind, state, timing, and its one-line
  result: the verdict with its counts for a compile; a missing outcome reads as died
  mid-run. Selecting a run pins it (`?run=`): a new job starting does not steal the
  view.
- The selected run: the transcript as session groups, newest session first, the
  running session pinned and highlighted with its tool calls streaming in. A session
  group is keyed by the event [label](../compiler/sessions.md#trace-events), one group
  per batch, so a run reads as its sessions, not as one interleaved stream. The header
  names the batch: its goals and their targets, and for a `reconcile-section` batch
  the document, its sections, and the section the session reached. A GC burst heads
  its own group with the `gcBurst` line.
- Inside a session, one card per round. The card header is the round's arithmetic:
  prompt size, response time, completion tokens, and how many tool calls the answer
  produced. Expanding it shows the round in full, fetched on demand
  (`GET /api/trace/{stem}/{n}`): the request (every message in the order it was sent,
  each collapsible), the response (the assistant message as it arrived, reasoning
  included, and the parsed tool calls), and the tool results the harness sent back. A
  retry or a sticky fallback shows as its own row with the error that caused it.
- The changesets the run committed (the journal entries whose build matches) render
  inline in order, each expandable to its mutations and reasoning, its resolved goals
  with their justifications, and the goals it opened with their causes.
- The whole-build report: one click opens the run's ripple
  (`GET /api/ripple?generation=` at the run's first generation) in the inspector: the
  causality DAG, the cost beside it, and the parked and failed goals with reasons.

### Preview

The preview pane shows the next session's prompt exactly as the model receives it,
assembled from the same code that runs the session
([the prompt](../compiler/sessions.md#the-prompt)): the agent contract, the active
skills, the `## Project` block, the `## Goals` block with each goal's contract paragraph
and hints, the `## Loaded` block, and the worker-protocol line. Beside it: the batch's
toolset and the executor the batch resolves to
([executors](../compiler/project-settings.md#executors)).

- In `compile: manual`, the action sheet opens the pane before the release in
  Developer mode, and the pane's release button records the release
  ([modes and releases](../compiler/control-plane.md#modes-and-releases)). The pane
  re-renders as documents change until the release lands.
- From a board card, the pane shows the batch that goal would join, the same rendering
  as `jazyk preview <goal|target>`.
- The pane is read-only: prompts are assembled, never authored.
- A preview makes no LLM call. The transcript records the same rendering per round, so
  [Runs](#runs) shows after the fact what the preview showed before.

### Journal

The raw [journal](../compiler/graph.md#journal) entries per generation, the source
[History](#history) renders in product words: mutations, resolved and opened goals,
rounds, tokens, and the entry kind.

### Workers

The whole control-plane snapshot from `GET /api/workers`: the modes, every registered
worker with its heartbeat and held batch, the live leases, the gated goal counts, and
a release button per stage when gated goals exist.

### Map

The whole-graph map, drawn from the graph itself and never from the rendered files
under `<out>/diagrams/`:

- Nodes are typed: entities (with their stereotype as a badge), documents,
  requirements, and product files. Edges are typed too: the
  [derived relationships](../compiler/graph.md#derived-data) between entities, one
  arrow per direction-and-type group, drawn with UML notation (a hollow triangle for
  generalization, a hollow triangle on a dashed line for realization, a filled diamond
  for composition, a hollow diamond for aggregation, a plain line for association, an
  open arrow on a dashed line for dependency, and the same labeled `«instantiate»` for
  instantiation), with cardinality labels where a contribution states one; a
  requirement to the entities it names; a requirement to the document its source
  anchors in; a requirement to the product files whose sites implement it.
- Containment draws as nesting ([containment](../compiler/model/entity.md#containment)).
  A parent collapses to one node showing its child count. With the children hidden,
  every relationship touching a hidden descendant lifts to the parent: one arrow per
  direction and type, promoted to the strongest type in the group, with a count label
  ([lifting and collapse](../compiler/diagrams.md#lifting-and-collapse)). Clicking a
  lifted arrow lists the concrete edges beneath it in the inspector. Lifting is
  computed on the server by the same code the emitters use, so the map and the
  rendered picture never disagree.
- Views as overlays: `?view=` draws that view's membership and nothing else, the
  structural kinds as the map, the flow kinds as ordered steps with the participants
  as lanes, a state view as its machine. A view over its hard limit draws with
  auto-collapse of the largest subtrees and the same visible note the rendered picture
  carries ([over-limit views](../compiler/diagrams.md#over-limit-views)).
- Type chips filter which node types draw; the default shows entities and documents.
  When two visible nodes are joined only through hidden ones, a collapsed tie draws
  between them, thin and dotted; selecting it inspects the intermediary.
- Focus: selecting a node and focusing pulls in every adjacent node of every type,
  chips notwithstanding, to one or two hops. Entity scope, stereotype, and edge-type
  filters apply.

### Benchmarks

The benchmarks page grades and compares models
([benchmark](../benchmark/benchmark.md)):

- The table merges three sources, latest per model and codec: results embedded in the
  binary (`source: embedded`), the machine-wide history (`~/.jazyk/benchmarks/`), and
  the project's own `results.yaml`. Columns are the workflow verdicts, the tier
  scores, efficiency, tokens, and throughput; rows with a different `caseSetHash` than
  the running binary's are marked stale.
- A run form: the endpoint URL (default: the resolved LLM settings), a model picked
  from the endpoint's `/v1/models` listing or typed free-form, and a run button that
  starts a benchmark [job](#jobs). Progress streams like any job; the finished grade
  lands in the table and the history.

API: `GET /api/benchmarks` (the merged table), `GET /api/benchmarks/models?baseUrl=`
(the endpoint's model listing), and `POST /api/jobs` with
`{kind: benchmark, baseUrl?, model?}`.

### Feedback

The history of the [feedback tool](../compiler/tools.md#feedback-tool): what the
models reported about jazyk itself, newest first. This page is for jazyk's developers,
not for the project's authors; nothing here is a statement about the documents.

- Each entry shows its kind, its subject, the message, and the references that name
  the caller: the source, the goal kind and batch, the target, the MCP client, the
  model, the codec, and the store generation.
- The kind is a filter: `?kind=` selects one, and the counts sit beside the filters.
- An entry made during a traced run links to that run under [Runs](#runs).
- A feedback call mid-build refreshes the page as it lands.

## Empty states and failure

- Every panel has an empty state that says what fills it and offers the action: a
  project with no sync yet offers Sync; a Product lens with no product folder offers
  the settings; an Inbox with nothing waiting says so.
- A slow backend shows a skeleton for at most two seconds, then a banner naming the
  request still pending. A read that another process blocks (the store busy under a
  build in another process) says which process holds it, from `GET /api/workers`.
- A dropped event stream banners the lost connection and polls until it heals; a
  rejected token gives way to the token prompt ([serving](#serving)).
- The default mode never renders a run list, a transcript, or a dead run. A failed
  sync is one line with its reason and `Details`.

## Frontend

The app lives in `bootstrap/app` and its built output (`bootstrap/app/dist`,
committed) embeds into the binary. The stack: React, TypeScript, Vite, TanStack Query
for server state, zustand for the ephemeral client state (connection, jobs, the trace
ring, the Developer toggle), CodeMirror 6 for the editor, react-router, react-markdown,
Radix primitives for menus, dialogs, sheets, and tooltips, and cytoscape for the
Developer map. One token file holds the design system: the type scale, spacing, and
the severity and status colors the [viewer](./viewer.md) uses (red `error`, amber
`warning`, blue `info`, grey `none`). The vocabulary table above is one module keyed
by the Developer toggle, so a label is never written twice.
