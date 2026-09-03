# GUI

`jazyk gui [--port N] [--no-open] [--watch] [--gui-dist DIR] [--no-token]` starts one
local process: an HTTP server that serves the GUI web app, a JSON API over the
[graph store](../compiler/graph.md), a live event stream, and the
[language server](./lsp.md) over WebSocket. It then opens the browser at the served
address. The GUI is the workbench for developing a jazyk project: edit the documents,
watch the compiler resolve goals, review the graph and its projections, review what
changed and why, and run [generation](../consumers/gen.md) and verification.

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
  directory, the [executors](../compiler/project-settings.md#executors), and the LLM
  model and base URL. The API key is never served.
- `GET /api/status`: `status.yaml` (the store version, the generation, the
  [verdict](../compiler/compilation.md#convergence) with its counts, the costs) plus
  node counts, the coverage fraction, open diagnostics by severity, and the board
  counts. The same summary as `jazyk status`.
- `GET /api/overview`: the same summary without the board counts, plus verification
  counts by [derived status](../consumers/gen.md#status-is-derived-never-stored).
  The viewer-style rollup, served live.
- `GET /api/graph`: every shard (entities, requirements, views, relationships, state
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
  reconstructed by replaying the journal. This powers the change review views. The
  journal entries between two builds are the release diff (see
  [release diffs](../consumers/pm.md#release-diffs-from-the-journal)).
- `GET /api/docsgen/{slug}`: the rendered per-entity requirements document.
- `GET /api/feedback?limit=`: the [feedback log](../compiler/tools.md#feedback-tool),
  newest first, capped (default 200, maximum 2000). Each entry carries the model's
  report plus the references that name its caller. An unreadable or absent log is an
  empty list, never an error.

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
  executor it resolves to. With `goal=`, the batch that goal would join. A preview
  makes no LLM call and spends nothing.
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
  generation package a session receives.
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
  newest first, capped at 200: each with its stem, its metadata line, its outcome
  when the run finished (a missing outcome means it died mid-run), and its event
  count. The [activity panel](#activity)'s run list.
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

## Layout

One workbench page. Navigation swaps panes, never the page. Six regions:

- The rail: a narrow icon strip on the far left: `files`, `graph`, `board`, `work`,
  `benchmarks`, `feedback`, `settings`. A rail item picks what the sidebar shows; it
  never navigates away.
- The sidebar: the navigator for the active rail item. Clicking an entry opens it in
  the center.
- The center: the open item. The document editor, the deliverable viewer, the map,
  the board, the work views, the settings form.
- The inspector: the detail pane beside the center. Selecting a node anywhere (a code
  lens, a map node, an arrow, a list row, a goal card, an id chip) shows its detail
  here, beside the center, never replacing it. Closable; the center keeps its state
  under it.
- The chat pane: the persistent pane on the far right, collapsible to a strip. The
  conversation surface: chat sessions with the agent and follow views of automated
  work, each with its loaded-set panel. See [chat](#chat).
- The activity panel: the bottom strip, always present. Collapsed it is one line:
  the run controls and the live build state. Expanded it is the run history and the
  selected run's transcript. See [activity](#activity).

Addressable state: `/files/docs/<path>`, `/files/deliverable/<path>`,
`/files/out/<path>` (a file under the out directory), `/graph`,
`/board`, `/work`, `/feedback`, and `/settings` pick the center; `?node=` holds the
inspector selection (any node id, a relationship, a state machine, or a goal);
`?view=` the view overlaid on the map; `?entity=` the [explorer's](#explore)
position and `?detail=` its detail under the overlaid view; `?goal=` the selected
board card; `?run=` the selected run. A document takes `?section=` and `?quote=` to reveal and highlight a
quote; a deliverable file takes `?site=<requirement>` to reveal that requirement's
first located site, or `?line=` to reveal a line directly.

### Files

One explorer over the project's trees, in labeled sections:

- Documents: the docs tree. Each document shows its open diagnostics as a
  severity-colored badge, its open goals as a count, and a drift dot when it is stale
  against the graph. Documents can be created, moved, and deleted from the tree,
  through the documents API and its validation; a delete asks for a second click,
  never a dialog. Directories exist implicitly through paths.
- Deliverable: the generated product files, each with its ownership count badge
  (the entities and requirements the ledger binds to it) and a stale dot when a
  bound requirement's verification is stale.
- Generated: the pages [docsgen](../consumers/docsgen.md) writes under
  `<out>/docsgen/` (the requirements documents, the entity cards, the level pages,
  the diagram pages), listed through `GET /api/out/list`. A row opens the page
  read-only in the center with its [markdown preview](#markdown-preview). The list
  refreshes when a build commits, since the pages render on every commit.
- Build progress: while a build runs, the documents it is working on say so in
  place. A document whose sections carry open `reconcile-section` goals is dimmed with
  a waiting mark; the document whose sections a session is reconciling shows a running
  mark, the section the session reached, and how many of the batch's sections it has
  touched. When the session ends, the row turns into its result (what was staged, or
  the failure) and fades a few seconds later. Hovering the row holds the result until
  the pointer leaves. The states come from the
  [trace events](../compiler/sessions.md#trace-events) of the running job, so a build
  started outside the GUI moves the lock, the counter, and the board as always, but
  does not light the tree up: its events are in its own transcript, not on this
  server's stream.
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
- A markdown deliverable file opens on its [markdown preview](#markdown-preview),
  with a `source` toggle back to the viewer.

### Markdown preview

A markdown file opens rendered, not as its raw text: headings, lists, links, inline
and fenced code, tables, block quotes, and images, in the app's palette. Which
surface holds the preview depends on the tree the file lives in:

- A generated page under the out directory (`/files/out/<path>`, the
  [generated section](#files)) and a markdown deliverable file open on the preview in
  place of the source; a `source` toggle swaps the read-only text in. Any other file
  under the out directory (a `.puml`, a `.yaml`) opens as read-only text, and a
  rendering (`.svg`, `.png`) opens as the image.
- A source document under the docs glob keeps its [editor](#editor) and shows the
  preview beside it, split, live as one types; a `preview` toggle on the editor's bar
  shows or hides it, and the choice is remembered in the browser.

Links and images resolve against the open file's path, so the pages walk the way
[docsgen](../consumers/docsgen.md#entity-cards) writes them:

- A relative link to a `.md` under the out directory opens that page in the center
  (a card's siblings, its diagram pages, its requirements document). A relative link
  to a document under the docs glob opens the editor on it, and one to a deliverable
  file opens the deliverable viewer. A `#fragment` scrolls the preview to the heading
  with that slug, under the compiler's own heading slug rule, the one docsgen writes
  its fragments with. A link with a scheme (`https:`, `mailto:`) opens in a new
  tab. A relative link that lands nowhere the GUI serves renders as plain text.
- A relative image under the out directory (`../../diagrams/class/checkout.svg` from a
  card) renders inline through `GET /api/out/file`, so the diagrams docsgen embeds
  show on the page. An image anywhere else is not served and shows its alt text.
  The images are the build's renderings, read as files; the map still draws the
  graph itself ([graph](#graph)).

The preview renders the text as it stands and never rewrites it: the editor's text
stays plain markdown, byte for byte, and a generated page is never written through
the GUI.

### Graph

The `graph` rail item is the whole graph surface: the sidebar navigates it, the
center draws it. The map renders its projections straight from the graph and never
reads the rendered files under `<out>/diagrams/`; those are build output for the
other surfaces ([diagrams](../compiler/diagrams.md#rendering)), and the
[markdown preview](#markdown-preview) shows them only where a generated page embeds
one.

- The sidebar: one text filter plus facet lists, the viewer's cards served live:
  entities, requirements, views, diagnostics (with triage actions), coverage.
  Views are grouped by [kind](../compiler/model/view.md#kinds), default views marked
  as such, each with its member count and its limits state. Suppressed diagnostics
  never render. A row opens the node in the inspector and focuses it on the map; a
  view row overlays the view.
- The containment tree: the entity list in the sidebar is the tree the `parent` field
  makes ([levels](../compiler/concepts/levels.md#levels)), one root per scope
  ([the scope root](../compiler/concepts/levels.md#the-scope-root)), each node
  expandable to its children. A node with a level shows its child count and its
  [level view](../compiler/diagrams.md#level-views) ids; clicking one overlays that
  view. A grouping (an entity with `derived` provenance) is marked as such, with its
  ratification proposal one click away.
- The breadcrumb: while a level view is overlaid, a breadcrumb sits over the diagram
  panel: the chain from the scope root down to the level's node. Each crumb overlays
  that ancestor's level view; the last crumb is the current level. A member drawn
  with a level of its own is the link down ([drill-down](../compiler/concepts/levels.md#drill-down)):
  double-clicking it, or picking it from the view's `children` list
  (`GET /api/views/{id}`), overlays the member's level view and extends the
  breadcrumb. `?view=` holds the overlaid view, so a drilled-down position is an
  addressable URL. Clearing the overlay clears the breadcrumb.
- The center: the map. Nodes are typed: entities (with their stereotype as a badge),
  documents, requirements, and deliverable files. Edges are typed too:
  - The [derived relationships](../compiler/graph.md#derived-data) between
    entities, one arrow per direction-and-type group, drawn with UML notation: a
    hollow triangle for generalization, a hollow triangle on a dashed line for
    realization, a filled diamond for composition, a hollow diamond for aggregation,
    a plain line for association, an open arrow on a dashed line for dependency, and
    an open arrow on a dashed line labeled `«instantiate»` for instantiation.
    Cardinality labels sit at the ends where a contribution states one.
  - A requirement to the entities it names (membership).
  - A requirement to the document its source anchors in.
  - A requirement to the deliverable files whose ledger sites implement it.
- Containment: an entity's children draw nested inside it
  ([containment](../compiler/model/entity.md#containment)). A parent collapses to one
  node showing its child count. With the children hidden, every relationship touching
  a hidden descendant lifts to the parent: one arrow per direction and type, promoted
  to the strongest type in the group, with a count label
  ([lifting and collapse](../compiler/diagrams.md#lifting-and-collapse)). Clicking a
  lifted arrow expands it: the concrete edges beneath it list in the inspector, each
  walking to its requirement and its sentence. Lifting is computed on the server by the
  same code the emitters use, so the map and the rendered picture never disagree.
- Views as overlays: selecting a view (the sidebar, `?view=`, or the inspector's
  overlay action) draws that view's [membership](../compiler/model/view.md#membership)
  and nothing else: the members, every relationship among them, lifted where the view
  hides descendants, the view's `collapse` set applied. Excluded members list in the
  inspector with their notes. The kind decides the drawing: the structural kinds
  (class, object, package, component, composite, deployment) draw as the map; the
  flow kinds (use-case, activity, sequence, communication, timing, overview) draw as
  ordered steps with the participants as lanes, one step per member requirement; a
  state view draws the derived [state machine](../compiler/model/state-machine.md#rendering)
  of its subject. A view over its hard limit draws with auto-collapse of the largest
  subtrees and the same visible note the rendered picture carries
  ([over-limit views](../compiler/diagrams.md#over-limit-views)). Clearing the overlay
  returns to the whole graph.
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
- Selecting an arrow opens its relationship in the inspector: the contribution group
  and its requirements.
- Entity scope, stereotype, and edge-type filters carry over from the overview.

### Explore

The walk the [cards](../consumers/docsgen.md#entity-cards) give a markdown reader,
the GUI gives live, from the same model (`GET /api/entities/{id}/card`, the card as
JSON, and `GET /api/views/{id}/page`, the diagram page as JSON). What the LSP cannot
do, this surface does:

- Click a node, anywhere: on the map, in an overlaid level view, in the tree, in a
  card, any entity id in the app. The inspector shows the entity's card first, the
  long detail under it: the definition, `Sits in`, the in-context view, `Inside`,
  relationships, flows, siblings, children, each item a link that moves the
  explorer. A diagram's nodes are live: clicking a box in an overlaid view opens
  that entity's card and, when it has a level, offers the level below. Closing the
  inspector clears the position; the overlaid view stays.
- Where the map goes: a move from a card (a sibling, a child, a relationship, a
  crumb) overlays the entity's context view (its parent's level, the scope root's
  for a parentless entity) with the entity selected, so the map shows where the
  entity sits. A click on a node already drawn (the map, the tree) keeps the
  overlay. The scope root has no card: its crumb overlays the root's level view.
- History: the explorer keeps a stack of positions (`?entity=` with `?view=`). Back
  and forward walk it; every move pushes, and a browser back lands on the same stack
  entry, so the two never disagree. The URL carries the position, so any point of
  the walk is addressable and shareable: a URL with `?entity=` and no `?node=` opens
  the inspector on the card.
- Detail: a level view overlays with its groupings collapsed; `more detail` expands
  every grouping one level (its children draw inside it, relationships lifted as the
  renderer lifts them), `less detail` collapses one level back up to the level's
  members. The control sits in the breadcrumb bar over a structural overlay, with the
  level count between its two buttons; `more detail` disables where the tree ends.
  The map redraws from the graph through `GET /api/views/{id}?detail=`; nothing is
  rendered by the build for this. `?detail=` holds it and resets when the overlaid
  view changes.
- Sideways: the card's siblings and the diagram page's `Around` list are chips; one
  click moves to the sibling entity or the neighboring view without going up first.
  `Around` sits in the breadcrumb bar: the level above (`↑`), the other views of the
  same level, and the levels below (`↓`, labeled by member).
- Up and down: the breadcrumb over the diagram panel stays; the card's `Inside` and
  `Sits in` are the same moves as chips.

### Board

The `board` rail item is the goal board: what compilation owes, why, and what it is
doing about it. The board is derived from disk on every consult
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
    gated release, each a link to where the human acts;
  - parked: out of budget, resumes first next build;
  - failed: the reason the session gave, standing on the target;
  - resolved: the card turns into its justification and stays until the build ends,
    then leaves the board; the journal keeps it.
- Cause lines: a card opened by a resolution draws a line from the card that resolved,
  while both show, and the line lights as the `goal.opened` event fires. The board is
  the ripple, live.
- A card click opens the live session in the chat pane when a session holds the goal
  (the [follow session](#chat)); otherwise it opens the goal in the inspector with its
  explanation (`GET /api/explain?target=`).
- Card actions: `preview` opens the [preview pane](#preview) on the batch this goal
  would join; `explain` and `ripple` open the inspector; a blocked `answer` card jumps
  to its question in the [questions list](#questions), a blocked `ratify` card to its
  proposal; a `split-view` or `abstract-entity` card offers `dismiss`, which stages
  [`bump_limit`](../compiler/graph.md#mutations) on the node (through
  `POST /api/facts/{id}/edit` with `{field: limits.<limit>, value}`): a decree that
  raises the node's own threshold ([per-node bumps](../compiler/graph.md#per-node-bumps)).
  Dismissal is a graph write, never goal state, and the goal stops deriving until the
  raised threshold is crossed.
- The board never shows decompilation: drafts stay outside the goal board
  ([work](#work)).

E.g.:

```
abstract-entity   ent:order                      GC · optional (mandatory at 80)
54 requirements > 50 (threshold-crossed)
cause: g412 #3 via requirements-per-entity
waiting: 2 compile goals open in the cone
hints: load ent:order · skill abstraction
```

### Preview

The preview pane shows the next session's prompt exactly as the model receives it,
assembled from the same code that runs the session
([the prompt](../compiler/sessions.md#the-prompt)): the agent contract, the active
skills, the `## Project` block, the `## Goals` block with each goal's contract paragraph
and hints, the `## Loaded` block (the loaded set as its status block), and the
worker-protocol line. Beside it: the batch's toolset and the executor the batch
resolves to ([executors](../compiler/project-settings.md#executors)).

- In `compile: manual`, the pane opens before the release: the compile click shows the
  next batch's prompt, and the pane's release button records the release
  ([modes and releases](../compiler/control-plane.md#modes-and-releases)). The pane
  re-renders as documents change until the release lands.
- From a board card, the pane shows the batch that goal would join, the same rendering
  as `jazyk preview <goal|target>`.
- The pane is read-only: prompts are assembled, never authored. Changing what the model
  will see means changing the documents or the graph.
- A preview makes no LLM call. The transcript records the same rendering per round, so
  the [activity panel](#activity) shows after the fact what the preview showed before.

### Work

The ledger-side worklists, opened from the `work` rail item:

- The verification matrix: every ledger row with its derived status chip and the
  [staleness cascade](../consumers/gen.md#the-cascade) explained per row. Rows open the
  requirement in the inspector.
- The generation packages: for each entity with an open `generate` goal, the package a
  session receives (`GET /api/gen/package/{entity}`). The goals themselves are board
  cards; this view shows what the session will be handed.
- The [unclaimed report](../consumers/bind.md#the-unclaimed-report) and the decompile
  action. Decompilation is outside the goal board: the action records a decompile
  release for its scope and dispatches like compile and generate
  ([decompilation](../consumers/decompile.md#triggering)).
- Run actions submit jobs to the activity panel.

### Feedback

The history of the [feedback tool](../compiler/tools.md#feedback-tool): what the
models reported about jazyk itself, newest first. This view is for jazyk's
developers, not for the project's authors; nothing here is a statement about the
documents.

- Each entry shows its kind, its subject, the message, and the references that name
  the caller: the source, the goal kind and batch, the target, the MCP client, the
  model, the codec, and the store generation.
- The kind is a filter: `?kind=` selects one, and the counts sit beside the filters.
- An entry made during a traced run links to that run, which selects it in the
  activity panel (`?run=`), so the call sits back in the transcript it came from.
- A feedback call mid-build refreshes the view as it lands, not at the end of the run.

### Inspector

The detail pane for one node, opened from anywhere, layered over nothing:

- An entity: name, definition, scope, stereotype, parent (opens it) and children,
  attributes (each with its provenance), mentions (each opens the editor at the
  quote), the requirements referencing it, its relationships by direction and type,
  the views it belongs to (each with an overlay action), its derived state machine,
  the files implementing it, and its verification rollup.
- A requirement: the statement, its provenance (a quote opens the editor at the
  quote; `derived` lists the upstream nodes and the reasoning; `decree` names the
  author and the note), its entities, its edges with type and cardinality, its
  transition, its facets with their reasoning, its implementing sites (each opens the
  deliverable file at the located line), and its verification status.
- A view: kind, title, the members in order, the excluded members with notes, the
  query, the collapse set, provenance, its limits state, and the overlay action.
- A relationship (from an arrow): the members and the contribution groups, each with
  its requirements. A lifted arrow lists the concrete edges beneath it first.
- A state machine: the states, the initial state, and the transitions, each with the
  requirement that declares it and its open [checks](../compiler/model/state-machine.md#checks).
- A diagnostic: message, severity, subjects, reasoning, the triage actions, and its
  prompt when it carries one.
- A goal (from a board card): kind, class, target, change, cause, state, hints, and its
  explanation: the change record, the readiness sentence, what blocks it.
- Every node id anywhere in the app opens the inspector. The center never changes
  under it; the click-through from a requirement to its implementation is: open the
  inspector, then open a site.

Justification walks. Every rendered element walks to the sentence behind it, and the
inspector is the walk ([justification closure](../compiler/compilation.md#checks)):

- An arrow opens its relationship, the relationship lists its contributing
  requirements, each requirement shows its sentence, and the sentence opens the editor
  at the quote. A lifted arrow adds one step: the concrete edges beneath it.
- An object value opens the attribute and the example sentence it came from.
- A component box opens the statements on the entity.
- A walk that reaches a `derived` or `decree` fact ends on its open ratification
  proposal instead of a quote, with the upstream nodes beside it.

Editing facts. Fields in the inspector are editable in place: a definition, an
attribute value, a statement, an edge's type or cardinality, a transition, a view's
members. Saving goes through `POST /api/facts/{id}/edit`, the
[edit paths](../compiler/compilation.md#edit-paths):

- A quote-provenanced fact: the inspector shows the proposed sentence rewrite as a
  diff of the sentence in its document. Accepting commits the dual write (the graph
  mutation and the prose replacement in one changeset, the document not re-dirtied);
  the open editor on that document updates in place. Declining lands the edit as a
  decree with a ratification proposal.
- A derived or decree fact, or a fact added with no prose behind it: a decree. The
  ratification proposal appears in the [questions list](#questions) at once.
- A default view: any edit to it clears its `default` mark, and the recompute leaves
  it alone from that commit on ([default views](../compiler/model/view.md#default-views));
  the inspector says so before the save. The inspector offers no delete while `default`
  is set: `delete_view` refuses a default view. Any edit clears the mark, and delete
  becomes available.
- Downstream goals derive from the graph change like any other, and the board shows
  them as they open.

### Chat

The chat pane is the GUI's [ACP client](./acp.md) surface. One session list, two
session kinds:

- Chat sessions: a conversation with the configured agent, created in the pane. The
  session gets the [`chat` serving](./mcp.md#toolsets), so the agent can read and load
  the graph, revise requirements and edit facts through the
  [dual-write tools](./acp.md#dual-write-tools), claim goal batches through the
  lifecycle tools, and edit project settings. Prompting streams the agent's thoughts,
  messages, and tool calls into the transcript as they happen.
- Follow sessions: every [worker session](./acp.md#worker-sessions) a job runs
  registers as a read-only session, so watching a build is opening its session. A
  board card opens the session holding its goal. The transcript is the same rendering
  as a chat session: the agent's messages, its tool calls, their results. The header
  names the batch: its goals and their targets.

The pane's behaviors:

- The loaded-set panel: beside every session's transcript, the
  [loaded set](../compiler/context.md#the-loaded-set) as the serving renders it
  ([rendering](../compiler/context.md#rendering)): each loaded item with its size,
  what it shows and what stays unloaded behind a handle, the unload suggestions, the
  high-water mark, and the skill index line. The panel re-renders live on every
  mutating tool reply and in full on `graph_status`, the same cadence the model sees.
  Every item opens in the inspector; a handle shows its size estimate. The panel is
  read-only on a follow session; on a chat session it reflects what the agent loads.
- [Slash commands](./acp.md#slash-commands): the same catalog the IDE proxy
  advertises, completed in the prompt box from the advertised list. A command means
  the same thing in both frontends; here a build command runs through the job queue
  and streams its progress into the same session.
- The [build plan](./acp.md#plans) renders as a live checklist: one entry per goal
  batch, showing the batch's task and target, flipping as the build advances.
- Follow mode: a toggle that pins the transcript to the newest update and moves the
  editor along with the work. A tool call carrying a location opens the document or
  deliverable file in the center at that line, so the center shows what the agent is
  touching while the pane shows what it is doing. Pair programming, with the agent
  driving.
- Permission requests from chat sessions surface inline as option buttons
  ([permissions](./acp.md#permissions)). An unanswered request cancels with the
  prompt it belongs to. Worker sessions never ask; their policy answers.
- Transcripts persist in the [session store](./acp.md#session-store) under
  `<out>/sessions/`, so a reloaded page, and a restarted server, restore the session
  list and its history. A restored conversation has no agent behind it until it is
  prompted again, which opens a fresh agent session under the same id.

API: `POST /api/chat/sessions` creates a session, `GET /api/chat/sessions` lists
them, `GET /api/chat/sessions/{id}` returns one with its transcript and its loaded
set, `POST /api/chat/sessions/{id}/prompt` sends a prompt (progress streams over the
event stream), `POST /api/chat/sessions/{id}/cancel` cancels the open prompt, and
`POST /api/chat/permissions` answers a pending permission request with body
`{sessionId, id, optionId?}` (a missing `optionId` cancels the request). Updates
travel as `chat.update` events, elided like `job.trace`; permission requests as
`chat.permission`; session list changes as `chat.sessions`.

### Questions

Open diagnostics carrying a [prompt](../compiler/model/diagnostic.md#prompts) render
in two places:

- A questions list in the chat pane, above the session list while any are open: the
  question, one button per option, and a text box when the prompt accepts freeform.
  Answering posts to the same engine every frontend uses: an `edit` option applies
  and the finding disappears with the next event; other answers show
  `answer.status` as the [answer session](./acp.md#answer-sessions) handles them.
- Inline in the [editor](#editor): the GUI's editor rides the same
  [LSP](./lsp.md#capabilities) the IDEs use, so a prompted diagnostic's options
  appear as quick fixes on the anchored quote, plus the freeform input the GUI can
  offer where base LSP clients cannot.

Two goal kinds are blocked on this list, and the board says so on their cards:

- An [`answer`](../compiler/goals/answer.md) goal waits on an unanswered prompt. The
  human's answer resolves it; applying the answer runs as an
  [answer session](./acp.md#answer-sessions), not a goal, and the recorded answer is
  the cause of what it commits.
- A [`ratify`](../compiler/goals/ratify.md) goal waits on a
  [ratification proposal](../compiler/model/diagnostic.md#ratification-proposals): the
  sentence the documents should gain, rendered as the prompt's `edit` option.
  Accepting applies the dual write and flips the fact's provenance to `quote` in the
  same changeset; `retract` removes the decree. Either way the goal leaves the board.

Opening a project with standing errors, warnings, and proposals re-surfaces them here
without any action: the list reads from the graph, and the graph kept them.

### Activity

The bottom panel shows runs: a run is one job plus what it committed. Collapsed, the
panel is a single control line; expanded, it is two parts:

- The run list: newest first, live jobs and the transcripts on disk (CLI runs
  included), each with kind, state, timing, and its one-line result: the verdict with
  its counts for a compile. Selecting a run pins it: a new job starting does not steal
  the view.
- The selected run: the transcript as session groups, newest session first, the
  running session pinned and highlighted with its tool calls streaming in. A session
  group is keyed by the event [label](../compiler/sessions.md#trace-events), one group
  per batch, so a run reads as its sessions, not as one interleaved stream. The header
  names the batch: its goals and their targets, and for a `reconcile-section` batch
  the document, its sections, and the section the session reached. A GC burst heads
  its own group with the `gcBurst` line (`abstract-entity ent:order (54 > 50)`).
- Inside a session, one card per round. The card header is the round's arithmetic:
  prompt size, response time, completion tokens, and how many tool calls the answer
  produced. Expanding it shows the round in full, fetched on demand:
  - The request: every message in the order it was sent, each collapsible, the
    agent contract, the skills, the goals block, and the `## Loaded` status block as
    rendered that round included. The request is what the model was asked; nothing
    about it is inferred from the reply.
  - The response: the assistant message as it arrived, reasoning field included, and
    the parsed tool calls with their arguments.
  - The tool results the harness sent back, each with the full payload, the condensed
    status block on every mutating reply included.
  A retry or a sticky fallback (codec downgrade, streaming, dropped `temperature`)
  shows as its own row in the round, with the error that caused it.
- The changesets the run committed (the journal entries whose build matches) render
  inline in order, each expandable to its mutations and reasoning, its resolved goals
  with their justifications, and the goals it opened with their causes: the trace
  says what the model did, the changesets say what landed and what it set in motion.
- The whole-build report: one click from a compile run opens its ripple
  (`GET /api/ripple?generation=` at the run's first generation) in the inspector: the
  causality DAG, the cost beside it, and the parked and failed goals with reasons.
- The control line, visible even collapsed: the compile button (with the board
  counts: open goals and blocked), the generate button (with the open `generate` goal
  count), verify, the [compile mode](#workflow-modes) select, and the generation mode
  select. The
  running job shows its kind, its current batch, and its progress here; cancel is one
  click.
- The changeset timeline is addressable per generation, and the release diff between
  any two generations stays reachable from the panel (the journal range diff).

## Benchmarks

The benchmarks tab grades and compares models
([benchmark](../benchmark/benchmark.md)):

- The table merges three sources, latest per model and codec: results embedded in the
  binary (`source: embedded`), the machine-wide history (`~/.jazyk/benchmarks/`), and
  the project's own `results.yaml`. Columns are the workflow verdicts, the tier
  scores, efficiency, tokens, and throughput; rows with a different `caseSetHash` than
  the running binary's are marked stale. Grading per goal kind is deferred; the table
  shows what the harness grades.
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
lives in the [control plane](../compiler/control-plane.md)
(`control.yaml` in the out directory), where the internal loop, `jazyk monitor`, and
every agent over MCP read the same policy. A mode set in the GUI survives a restart
and binds the agents too.

`GET /api/watch` and `PUT /api/watch` carry `{compile, gen}`, each `auto` or
`manual`:

- `compile: manual` (the default): changes queue visibly. The board derives from the
  saved documents, so the goals a save opens appear as cards before any release,
  carrying `gated: true`; the control line counts the documents that drifted from the
  graph and the goals open. Compiling is an explicit click: the click opens the
  [preview pane](#preview) on the next batch, and the pane's release records a
  [release](../compiler/control-plane.md#modes-and-releases), so an attached agent's
  watcher fires from the same click.
- `compile: auto`: changes compile automatically, the loop of
  [`jazyk watch`](./cli.md#jazyk-watch): debounced events, a fingerprint gate,
  backoff retries for `incomplete` builds. No preview pane: the prompt is still on
  record in the transcript.
- `gen: manual` (the default): generation runs on click, which likewise records a
  release. The release covers [binding](../consumers/bind.md#when-binding-runs) too:
  open `bind` goals run before `generate` goals.
- `gen: auto`: a finished compile with open `generate` goals queues a `gen` job behind
  it ([incremental regeneration](../consumers/gen.md#incremental-regeneration)).

Decompilation has no mode: the decompile action is always an explicit click. It
records a decompile release for its scope and dispatches like compile and generate
([decompilation](../consumers/decompile.md#triggering)). The
[unclaimed report](../consumers/bind.md#the-unclaimed-report) beside the action shows
what territory has no docs; the count shrinks as drafts land and their statements
bind.

`--watch` starts with `compile: auto`. Automatic modes spend LLM budget, so both are
opt-in. With both automatic, a document change compiles and regenerates end to end;
the chain never loops, because generation does not touch the documents. Running
`jazyk watch` in a terminal beside the GUI is safe: builds serialize on the build
lease and commits on the store lock, and the second build derives an empty board.

### Workers

`GET /api/workers` reports the control plane: the modes, the registered
[workers](../compiler/control-plane.md#workers-and-leases) with their heartbeats and
held batches, the live leases, and the gated goal counts. The workers strip renders
it: who is attached ("claude-code agent, awaiting release", "working on
reconcile-section docs/orders.md"), and a release button per stage when gated goals
exist.

Compile and generate clicks dispatch by the `worker` preference
([dispatch](../compiler/control-plane.md#dispatch)): with an agent registered and
preferred, the click records the release and the agent claims the batches
([compilation over MCP](./mcp.md#compilation-over-mcp)), its progress streaming into
the [activity view](#activity) from the MCP transcript and onto the board from the
commits; otherwise the GUI runs its own job. `POST /api/release` with `{stage}`
records a release without running anything, the button the workers strip uses.

## Editor

The GUI embeds a code editor on the project's documents, backed by the language server
over `GET /lsp` (WebSocket, one JSON-RPC message per text frame, no Content-Length
framing). Each connection is its own session with its own open-document overlay. The
[capabilities](./lsp.md#capabilities) are the LSP's: anchored diagnostics, hover with
the most relevant view's picture and the entity's page link, the requirement card, go
to definition, references, completion, document links, code lens.

- Markdown renders inline while editing. Headings take their size, emphasis and
  inline code take their style, links show their text, list bullets and quote bars
  draw as marks, and fenced code highlights in its own language. The markup syntax
  (`#`, `**`, backticks, `](url)`) appears only where the selection touches it, so
  the document reads like a page and edits like text. The text itself stays plain
  markdown, byte for byte: the docs are compiler input, provenance quotes locate
  against the exact characters, and nothing is rewritten by rendering.
- The [markdown preview](#markdown-preview) sits beside the editor when its
  `preview` toggle is on, split with the text, and follows every keystroke. Its
  links resolve against the document's path: another document opens in the editor,
  a generated page under the out directory opens in the center, an image under the
  out directory renders inline.
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
- A build in progress is visible in the text. When a session takes this document's
  sections, the sections in its batch are banded as queued, and the section the
  session reached ([`section` events](../compiler/sessions.md#trace-events)) is banded
  as running. Each band marks its first line in the gutter, beside the coverage
  border. When the session ends, the bands become its result, green for committed and
  red for parked or failed, and clear a few seconds later. Hovering a band holds it,
  and its tooltip names the batch, the section, and the outcome.
  Section lines come from the last reconciled section tree, the same source as the
  coverage bands, so they can drift against unsaved edits until the next build
  commits.
- A dual write shows in the text as it lands: the accepted sentence rewrite replaces
  the sentence in place, the document does not re-dirty, and the gutter shows no
  change against the baseline for it, because the commit absorbed the new hash.
- The editor diffs against the reconciled baseline (`GET /api/docs/baseline`):
  changed, added, and deleted lines mark the gutter, updated live as the text
  changes. The marks answer what the next compile will see as dirty. A `diff`
  toggle swaps the editor for a side-by-side diff of baseline against current text;
  the current side stays editable. A never-reconciled document shows no marks and
  no toggle.
- Saving writes through the documents API with the conditional hash, so an edit made
  outside the GUI is never silently overwritten.
