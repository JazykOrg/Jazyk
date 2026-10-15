# Local editor API

[Binary documents](../compiler/binary-documents.md) define native DOCX and PDF
reading, binary saves, source locations, and the `binaryDocuments` capability.

A project opens all [document sets](../compiler/document-sets.md). Graph operations address one set
explicitly. Each set owns `.jazyk/<set-id>/`; project file operations share the
physical files and report every membership. The document-set contract defines
configuration, aggregate reads, management commands, and grouped compilation.

`jazyk gui [--port N] [--no-open] [--watch] [--gui-url URL] [--no-token] [--dev]`
starts a loopback HTTP API over the [graph store](../compiler/graph.md), a live event
stream, and the [language server](./lsp.md) over WebSocket. It opens `https://jazyk.dev`
with connection details. `--dev` asks the application to open its Debug mode.
The binary contains no page, frontend assets, or cloud account service.

The application source and its presentation contract live in the separate private
[Jazyk Cloud editor](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md).
The native API, CLI launcher, WASM engine, ACP, LSP, and MCP stay in this repository.
The engine builds and runs without the application repository. No cloud account
is needed for this local API. Another compatible application can use `--gui-url`.
The [viewer](./viewer.md) remains the independent offline snapshot.

The [protocol version](./protocol.md) allows editor and engine releases to evolve
independently. The client checks compatibility before adopting a connection.

## Serving

- The server binds `127.0.0.1` only. It is never exposed beyond the local machine.
- Without `--port`, the server tries ports `4680` through `4711`, inclusive, in
  ascending order. It takes the first available port and prints the actual URL.
  Only an address-in-use error advances to the next port. Other bind errors stop
  startup. When all 32 ports are busy, startup asks the user to stop an existing
  server or choose an explicit `--port N`. There is no ephemeral fallback.
- An explicit `--port` binds only that port. A busy port is an error.
  Explicit `--port 0` asks the operating system for an ephemeral port. The opened
  connection link carries the actual port.
- `GET /api/ping` answers without a token: the app name, version, and the served
  project root. A server per project is normal, so when the default port is busy the
  startup line probes the occupant and names which project it serves. A stale tab is
  self-evident too: the app titles itself with the project directory and shows the
  root in the status bar.
- The server serves the API alone. The app is the page at
  [jazyk.dev](../site.md#the-editor), and `jazyk gui` opens it at
  `https://jazyk.dev/#/connect?port=<port>&token=<token>`, so the page connects to
  the server on this computer ([connecting to a local jazyk](#connecting-to-a-local-jazyk)).
  Nothing of the app ships in the binary. `--gui-url URL` (or `JAZYK_GUI_URL`) opens
  another copy of the page instead (a `vite` dev server, a fork), and that origin is
  the one the server admits (a dev server named by `localhost` or `127.0.0.1` is
  admitted under both names, since a browser reaches it by either).
- The page runs on a different origin than the server, so every API response
  carries `Access-Control-Allow-Origin` for the admitted page origin only
  (`https://jazyk.dev` by default), the headers and methods the app uses,
  `Access-Control-Allow-Private-Network: true` for the preflight, and
  `Cross-Origin-Resource-Policy: cross-origin`, since the page is cross-origin
  isolated ([serving in a tab](./wasm.md#serving)) and would otherwise refuse the
  response. A request from any other origin gets no allow header and the browser
  drops it. The token still guards every request: the allow header opens the
  transport, never the project.
- The allow header stops a page from reading an answer, not from sending a request:
  a browser sends a cross-origin `POST` without reading the reply. So every request
  other than `GET`, `HEAD`, or `OPTIONS` passes an origin check too. A request whose
  `Origin` header names an origin the server does not admit is refused with `403`.
  A request without `Origin` whose `Sec-Fetch-Site` is neither `same-origin` nor
  `none` is refused the same way. A request carrying neither header (a local tool,
  `curl`) passes. The check holds with or without a token, so `--no-token` keeps
  `POST /api/shutdown` and the write routes out of reach of another site's page.
- The editor's WebSocket (`/lsp`) feeds one language server thread per connection
  through a bounded queue of 1024 frames. A frame arriving at a full queue is
  dropped, and the server logs the drop once per connection; the editor's next full
  text sync restores the state. A build notification that finds the queue full is
  dropped too, since the next one repaints the same documents.
- A request to a path the API does not serve answers `404`. The server serves no
  page.
- Each start mints a random session token, embedded in the URL the browser opens. Every
  API and LSP request carries it (`Authorization: Bearer` or a `token` query
  parameter). Requests without it get `401`. The token keeps other local processes from
  spending LLM budget or writing documents through the API. `--no-token` disables the
  check, for frontend development.
- The app reads the port and the token from the connect route once, stores them
  in `localStorage` under the port (`jazyk.server.<port>`), and drops the fragment so
  copied URLs do not carry the secret. A reload, a new tab, or a browser restart
  resumes the connection with the stored token, no fragment needed. A stale entry
  (the server restarted and minted a new token) surfaces as the token prompt, and a
  pasted URL or token replaces the stored one.
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

### Connecting to a local jazyk

The page at jazyk.dev is one app with every backend in it: the demo, the browser
backend over a folder or browser storage, and a server on this computer. Connecting
to the server is a route, `#/connect?port=<port>&token=<token>`, the one `jazyk gui`
opens:

- The page pings `http://127.0.0.1:<port>/api/ping`, reads the project's name and
  root from the answer, stores the token under the port, adds the server to
  `Recent` as `<name> · jazyk on this computer`, and opens the documents. Every
  request from then on goes to that server: the API over `fetch`, the events over
  `EventSource`, the editor's language server over `ws://127.0.0.1:<port>/lsp`. The
  browser allows a secure page to reach `127.0.0.1`, since the loopback address is a
  trustworthy origin.
- Home's `Connect to jazyk on this computer` card explains the same thing for a
  person who arrived without the link: install the CLI, run `jazyk gui` in the
  project's folder, and the page opens itself; a port field (default `4680`) and a
  `Connect` button reach a server already running, and ask for its token when the
  ping answers but the API refuses.
- Automatic discovery checks all 32 ports in the default range and remembered
  custom ports through `/api/ping`. It does not stop after consecutive misses.
  Servers can stop in any order, leaving unused ports before a running project.
  The bounded range makes discovery complete for default launches without scanning
  arbitrary ports. Discovering a project never grants its session token.
- A server that stops surfaces as the lost-connection banner, as before; a server
  that restarted surfaces as the token prompt.
- The page works without a server: the demos, browser projects, and folders need
  none. The server adds what a tab cannot do: every job kind, the language server,
  an agent, rendering, and the disk.

## API

All endpoints live under `/api` and speak JSON. Reads load the store the same
[lock-free way](../compiler/graph.md#concurrency) every frontend does. Node ids resolve
through [redirects](../compiler/graph.md#mutations).

Project and store reads:

- `GET /api/project`: project root, out directory, docs glob, roots, deliverable
  directory, the [executors](../compiler/project-settings.md#executors), and the LLM
  model and base URL. The API key is never served.
- `GET /api/status`: `status.yaml` (the store version, the generation, the
  [verdict](../compiler/compilation.md#convergence) with its counts, the costs, the
  `parked` goals as goal records each with its `reason`, the `failed` goals) plus
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
  drafts, plus the sum the top bar shows, and the ids each section lists
  (`sections`).

The board and causality:

- `GET /api/board`: the [board](../compiler/reconciler.md#goal-derivation) as the
  reconciler derives it from disk: every open, blocked, parked, and failed goal with its
  kind, class, `mandatory`, target and unit, `change`, `cause`, `state`, hints, its
  [readiness](../compiler/reconciler.md#readiness) (ready, or the blocking reason as a
  sentence), for a parked goal the reason the build parked it (`parkedReason`,
  [parked and failed](../compiler/reconciler.md#parked-and-failed)), and the batch id
  when a running session holds it. The ready goals come
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
- `GET /api/docs/content?path=`: the raw document text, its hash, and
  `linkDiagnostics` from the current text and project files. Each finding contains
  `rule: "broken-link"`, `severity: "warning"`, `target`, `href`, `message`, and
  `range` with zero-based `{line, character}` start and end positions in UTF-16.
  Every missing link occurrence has a range. These read-only findings need no
  compiled graph or model call and disappear when the target file exists.
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
  matches the glob but does not exist yet creates a new document. A null or omitted
  `baseHash` is create-only: an exclusive filesystem create refuses an existing
  file with `409`, including a file created after the caller's read or one whose
  bytes cannot be read as UTF-8. It never replaces that file. A save that changes
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
  node pages use. `id` is a node id; `field` is a field of it (`definition`, an
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
  counts. A row carries its criteria's tests (`criteria`, by criterion id: the
  test's name, artifact, and kind, its verdict, and its last run), its implementing
  `files`, and its anchored `sites` (file and line), as the
  [ledger](../consumers/gen.md#the-ledger) records them. `?format=csv` or `?format=md` answers the same rows as a file for
  download ([trace matrix](#trace-matrix)).

Settings:

- `GET /api/settings`: the [project settings](../compiler/project-settings.md) as
  parsed from `jazyk.toml`: which keys the file sets, the effective defaults for the
  rest, and the file hash. The API key is reported only as set or unset, never its
  value.
- `GET /api/settings` also returns `text`: the settings file as written, for the
  editor's text mode. A stored `llm.api_key` value reads as `"(stored)"`; the key
  never reaches the client.
- `PUT /api/settings` with `{baseHash, text}`: write the settings file as given.
  The text must parse and describe valid document sets, else 400. An
  `llm.api_key` still reading `"(stored)"` keeps the stored key. A stale
  `baseHash` is 409.
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
  `.jazyk*` directories are skipped, so a deliverable at the project root lists the
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
- `GET /api/deliverable/raw?path=`: one product file as its bytes, with the content
  type its extension names (the out directory's types below, plus `image/jpeg`,
  `image/gif`, `image/webp`, `application/pdf`, and the Office types for `.pptx`,
  `.docx`, and `.xlsx`). This is what the [overview](#overview) renders a built
  artifact from and what its `Download` saves. The same confinement as the file read.
  A download link carries no bearer header, so the `token` query parameter authorizes
  it.
- `GET /api/product/overview`: what the [overview](#overview) shows. `kind` is
  `written`, `built`, `program`, or `none`; the reply carries the recorded `medium`
  and `build`, the test run `commands` (the first three distinct, `moreCommands`
  counting the rest), and per kind: `parts` (each prose file's path and text, in
  reading order, with `order` saying `index` or `names`), `artifacts` (each path with
  whether it `exists`, its `size`, and its content type), or `readme` (its path, the
  tree it came from, `deliverable` or `docs`, and its text).

The out directory:

- `GET /api/out/file?path=`: one file under the out directory, as its bytes with the
  content type its extension names (`image/svg+xml` for `.svg`, `image/png` for
  `.png`, `text/markdown` for `.md`, `text/plain` for `.puml`, `.yaml`, `.txt`,
  `.json`, and `.jsonl`, the types `GET /api/deliverable/raw` adds, and
  `application/octet-stream` otherwise). Read-only, no write
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

- `GET /api/project/files`: every file under the project directory (the folder of
  `jazyk.toml`), recursively, each `{path, size}` relative to it, dot-folders and the
  out directory included. `.git` and files whose names start with `.env` are left
  out. `node_modules` and `target` are omitted except for files matching a saved
  task's `products` globs. These retained products can be read, but the ordinary
  restrictions on editing build folders still apply. Secret paths and symlinks
  to secret paths cannot be exposed through product globs. Beside
  the list: `deliverable` and `out`, the product and out directories relative to the
  project directory (absent when outside it), and `docsGlob`. The
  [navigator](#workspace) draws its tree from it.
- `GET /api/project/file?path=`: one file under the project directory, as its bytes
  (in a tab, as text; a file that is not text is 501). A path that leaves the
  project directory is 400, a path that names nothing 404. A `.env` file is 400,
  like a skipped folder. A `jazyk.toml` is served with every `api_key` value
  replaced by `<redacted>`: the key is never served, the same rule as
  `GET /api/project` and `GET /api/settings`.
- `GET /api/project/text?path=`: one existing project file as `{path, text, hash}`,
  for editing. A file that is not UTF-8 text is 415. `PUT /api/project/text?path=`
  with `{text, baseHash}` replaces it when `baseHash` still names the file on disk
  (else 409), keeping its permissions. Every project file can be edited this way,
  inside a document set or not, under the rules of the next item, except that
  dot-folders other than `.jazyk` and `.git` are allowed. A `.env` file or a
  `jazyk.toml` cannot be written this way (400): the settings file is edited
  through `/api/settings`, which never serves the key.
- `PUT /api/project/file?path=` with `{text}` or `{base64}`: create one new file
  anywhere under the project directory, inside a document set or not. It never
  overwrites: an existing path is 409. Missing folders are created. A path that
  leaves the project directory, crosses a symlink or a nested project, or names a
  dot-folder (`.jazyk` included), `.git`, `node_modules`, `target`, the out
  directory, a `.env` file, or a `jazyk.toml` is 400. A DOCX or PDF must be a valid package (422). The limit is 32 MiB
  (413). The new file gets ordinary permissions. Edits to an existing document go
  through its document endpoints.
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

The local API server runs builds and workers itself. `POST /api/jobs` with
`{kind: build | compile | gen | verify | audit | decompile | benchmark}` (plus targets and `force` where
the kind takes them) queues a job and returns its id. `build` is the whole pipeline
for one document set in one job: compile, then generate when entities wait to be
generated, then review when the ledger holds generated code. A failed step ends
the job; a cancelled one stops at the next step. Its result lists each step it
ran. A worker preference that dispatches to an attached agent treats `build` as
a compile release. `GET /api/jobs` lists jobs,
`GET /api/jobs/{id}` returns one job with its state, result, and its whole trace: the
server keeps every event a job emitted, numbered per job, so a reloaded page shows the
same history the live stream showed. `POST /api/jobs/{id}/cancel` requests
cancellation.

- Every job's trace persists as one JSON-lines file under `<out>/trace/`: a metadata
  line, one line per numbered event, and a final line with the outcome. The activity
  panel lists past jobs from these files, so the history survives server restarts and
  page reloads, and any tool can load a transcript programmatically. Files older
  than 30 days are removed when the server starts.
- Retention in memory is bounded: the server holds the queued and running jobs and
  the 50 most recent finished jobs, events included. An older finished job leaves
  memory: `GET /api/jobs` no longer lists it, and `GET /api/jobs/{id}` answers from
  its transcript file, which holds every event already, so the job reads the same
  either way (state, result, and the numbered events, elided like the stream). A
  job whose transcript was removed is `404`. The transcripts under `GET /api/trace`
  are the record of past runs; the job list is the record of this server's current
  work.
- In the automatic compile mode a compile that ends `incomplete` (work parked, e.g.
  a transient endpoint outage) retries with the backoff `jazyk watch` uses: 30
  seconds doubling to 5 minutes, kept per document set for the life of the server,
  reset by a compile that completes or by a document change.
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
- A job whose run panics ends `failed` with the panic message as its result, and the
  worker goes on to the next job: nothing stays `running`.
- Stop cancels the active session and discards its staged changes and model context.
  Completed session commits remain. Cancellation prevents late replies and implicit
  end-of-input finishes from committing the discarded session. A commit already
  accepted remains atomic. The job reports `cancelRequested` while stopping and
  `cancelled` after cleanup. The provider request is cancelled where supported;
  otherwise its eventual reply is ignored.
- Stop clears queued follow-up jobs for the same document set and suppresses automatic
  builds and retries until an explicit Build. It does not change workflow settings or
  discard document edits. The next Build reads saved documents and derives remaining
  work from the retained graph. Cancellation does not consume a failed-session retry.
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
  refetches `GET /api/board` for the cards. Derivations coalesce per document set:
  the causes landing within about 300 ms of each other produce one derivation and
  one event, and one derivation runs per set at a time, so a burst of commits or a
  lease heartbeat never queues work behind itself.
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
  freed. Carries the workers snapshot the workers strip renders. The event is
  scoped: one per document set whose control files moved, `setId` naming it, and
  that set's board re-derives behind it.

External activity surfaces the same way: a `jazyk compile` run from a terminal, or an
[MCP](./mcp.md) agent committing through write tools, moves the lock and the counter,
and the GUI renders it live without owning the job. The goal events come from the
journal entries at commit, not from the job, so a build run anywhere moves the board.

## Backends and capabilities

One app, three backends, chosen when a project opens, never at build time. The
[server](#serving) (`jazyk gui`, reached from the page over
[a connection](#connecting-to-a-local-jazyk)) serves everything; the demo runs a
bundled example in the tab; the [browser backend](./wasm.md) runs the compiler in
the tab over a folder on the computer or browser storage. The backends do not change
the layout. They change what the backend can do, so the app reads one descriptor and
renders against it:

- `GET /api/capabilities`:
  - `projects`: `list` (the server's recent projects and a path input), `bundled`
    (the demo's examples), `folder-picker` (the browser backend's File System Access
    flow).
  - `docs`: `read` or `write`.
  - `events`: `stream` (the [event stream](#events)), `pushed` (trace events and
    chat events posted from work running in the page), `refetch` (no stream; the app
    refetches after every write).
  - `lsp`: whether `GET /lsp` exists. Without it the editor derives the same
    decorations from the graph's mentions and quotes on the client.
  - `jobs`: the job kinds the backend runs, from `compile`, `gen`, `verify`, `audit`,
    `decompile`, `benchmark`.
  - `agent`: `embedded`, `external`, or `none`. Without an agent the chat column
    shows build threads only and says why. In a tab, `embedded` is the
    [chat loop in the worker](./wasm.md#chat) on the page's model. It holds whether
    or not that model is ready; `model` says which, and while it is not `in-page` the
    chat column offers the model chooser in place of the prompt box.
  - `model`: `configured`, `in-page` (a model loaded in the tab), `loadable` (the
    tab can load one, on request), `needs-key`, or `none`.
  - `modelTier`: the loaded model's tier, `build`, `chat`, or `reader`
    ([model sources](./wasm.md#model-sources)), or `null` with no model; an endpoint
    and the server's model count as `build`, the browser's built-in model as `reader`.
  - `rendering`: `server` or `none`. A tab renders no `.svg`; a demo seed ships its
    renderings.
  - `persistence`: where a change goes.
    - `disk`: the server writes the project's files.
    - `folder`: a folder on the computer, through its directory handle, mirrored
      back from the tab ([a folder on disk](./wasm.md#a-folder-on-disk)).
    - `browser`: the browser's own storage, on this browser only: a
      [project started in the browser](./wasm.md#a-project-in-the-browser) or a
      demo, whose first edit keeps a copy ([browser storage](./wasm.md#browser-storage)).
      A reload keeps the changes.
    - `memory`: the tab only. The browser has no storage to offer, and a reload
      starts from the seed.
- A route the backend does not serve answers `501` with a reason
  (the browser backend's rule). A panel whose read answers `501` hides
  behind that reason; it never renders empty or errors.
- A missing capability hides its action and explains it in the empty state ("Build
  needs a model. Add one in Settings."). Nothing fails for want of a capability.
- The transport is one seam: every read and write goes through one
  `request(method, path, body)` the backend implements, so the app never knows which
  backend it has beyond the descriptor.

Where a project's changes go is said once, in the [project menu](#the-project-menu),
from `backend` and `persistence`, in one line: `Saved in this browser only`, `Saved
to <folder>` (with the mode), `Demo: edits are saved in this browser`, `Demo:
changes stay in this tab` (with `persistence: memory`), or `Saved on this computer
by jazyk` for a server.


## Editor specification

These presentation sections moved to Jazyk Cloud. Their anchors remain as pointers
for existing links. They are application behavior, not a compiler build dependency.

- <a id="product-surface"></a>[Product surface](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#product-surface).
- <a id="vocabulary"></a>[Vocabulary](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#vocabulary).
- <a id="presentation"></a>[Presentation](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#presentation).
- <a id="home"></a>[Home](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#home).
- <a id="the-setup-wizard"></a>[The setup wizard](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#the-setup-wizard).
- <a id="the-open-project"></a>[The open project](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#the-open-project).
- <a id="the-project-menu"></a>[The project menu](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#the-project-menu).
- <a id="layout"></a>[Layout](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#layout).
- <a id="the-action-button"></a>[The action button](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#the-action-button).
- <a id="pages"></a>[Pages](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#pages).
- <a id="docs"></a>[Docs](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#docs).
- <a id="model"></a>[Model](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#model).
- <a id="explore"></a>[Explore](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#explore).
- <a id="product"></a>[Product](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#product).
- <a id="overview"></a>[Overview](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#overview).
- <a id="product-file-viewer"></a>[Product file viewer](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#product-file-viewer).
- <a id="trace-matrix"></a>[Trace matrix](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#trace-matrix).
- <a id="not-described"></a>[Not described](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#not-described).
- <a id="markdown-preview"></a>[Markdown preview](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#markdown-preview).
- <a id="inbox"></a>[Inbox](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#inbox).
- <a id="questions"></a>[Questions](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#questions).
- <a id="proposals"></a>[Proposals](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#proposals).
- <a id="problems"></a>[Problems](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#problems).
- <a id="pending-changes"></a>[Pending changes](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#pending-changes).
- <a id="drafts"></a>[Drafts](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#drafts).
- <a id="history"></a>[History](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#history).
- <a id="chat"></a>[Chat](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#chat).
- <a id="inspector"></a>[Inspector](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#inspector).
- <a id="editor"></a>[Editor](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#editor).
- <a id="settings"></a>[Settings](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#settings).
- <a id="workflow-modes"></a>[Workflow modes](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#workflow-modes).
- <a id="workers"></a>[Workers](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#workers).
- <a id="workspace"></a>[Workspace](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#workspace).
- <a id="the-board"></a>[The Board](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#the-board).
- <a id="the-linkage"></a>[The linkage](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#the-linkage).
- <a id="developer-toggle"></a><a id="debug-toggle"></a>[Debug toggle](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#debug-toggle).
- <a id="board"></a>[Board](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#board).
- <a id="runs"></a>[Runs](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#runs).
- <a id="preview"></a>[Preview](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#preview).
- <a id="journal"></a>[Journal](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#journal).
- <a id="workers-1"></a>[Workers](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#workers-1).
- <a id="map"></a>[Map](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#map).
- <a id="benchmarks"></a>[Benchmarks](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#benchmarks).
- <a id="feedback"></a>[Feedback](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#feedback).
- <a id="empty-states-and-failure"></a>[Empty states and failure](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#empty-states-and-failure).
- <a id="frontend"></a>[Frontend](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/editor.md#frontend).
