# Browser Backend

The compiler compiles to WebAssembly. The same store, reconciler, board, goal
registry, tools, and [MCP serving](./mcp.md) the native binary runs also build for
`wasm32-unknown-unknown`, where they answer the [GUI](./gui.md)'s API from inside the
browser tab instead of from a process on a machine. The frontend talks to either.

The point is a jazyk that needs no install: a page that carries a project, a graph,
and a model, and reconciles one against the other while a person edits the prose.

## One API, two transports

The [GUI](./gui.md) API is one implementation with two ways in.

- `crate::webapi` holds the values: `status_value`, `board_value`, `preview_value`,
  `explain_value`, `ripple_value`, `views_value`, `view_value`, `tree_value`,
  `card_value`, `page_value`, and the [product](#the-product)'s
  `deliverable_value`, `deliverable_file`, `deliverable_baseline`, `matrix_value`,
  `matrix_export`, `gen_pending_value`, `gen_package_value`, `verify_pending_value`,
  `docsgen_value`, `out_list`, and `out_file`. Each takes a
  [store](../compiler/graph.md) (or a project and an out directory) and answers a
  JSON value, or a refusal with its status and reason. Nothing in it knows about a
  transport.
- `crate::gui` is the native transport: axum handlers, each a thin wrapper that loads
  the store off the async runtime and calls one of those functions.
- `crate::webapi::dispatch` is the browser transport: one `request(method, path,
  body)` that routes the same method and the same path to the same functions and
  answers the same JSON.

The frontend asks `lib/backend.ts` for a backend rather than calling `fetch`, so no
endpoint in `lib/api.ts` knows which one it has. A backend answers
`{ status, body }`; the server backend builds that from a `Response`, and the wasm
backend gets it from the module.

A route the browser backend does not serve answers `501` naming what it is, so the
app degrades one panel at a time instead of failing whole.

`GET /api/project` carries `backend: "wasm"` when the browser is serving, which is
how the app knows to hide what a tab cannot do.

## The seams

Three modules carry everything the two targets do not share. Each mirrors the
`std` surface the crate actually uses, with the same signatures and the same
`io::Result`, so a call site reads the same on both targets.

- `crate::vfs` is the filesystem. Natively it delegates to `std::fs`. In a tab it is
  an in-memory tree, one per module instance. `Path::exists`, `is_dir`, and `is_file`
  are inherent methods and cannot be intercepted, so they travel as
  `VPathExt::vexists`, `vis_dir`, and `vis_file`.
- `crate::sys` is the process and the clock: `run_shell`, `process_id`, `now_unix`,
  `now_millis`, `sleep`. A tab has no shell, so `run_shell` answers an error naming
  what is missing rather than pretending a command failed. `sleep` does nothing: the
  single thread the backend runs on is the one the page draws with.
- `crate::runner` is what runs a goal batch as a session. See
  [sessions](#sessions).

Modules that need a process, a socket, a terminal, or file watching are gated off the
wasm build in `src/lib.rs`: `acp`, `cli`, `gui`, `lsp`, `viewer`, `benchmark`,
`answer`, `parallel`, `jsonrpc`, `launcher`, `replay`.

### Filesystem

The tree holds paths and bytes. A directory is implied by any file under it, so a
project seeded as a flat list of files needs no directory records. The host fills it
before the backend boots:

- `reset()` empties the tree. The host calls it before seeding another project, so
  a second demo opened in the tab does not inherit the first one's files.
- `seed_file(path, bytes)` writes one file.
- `boot(root)` adopts the tree as the project and answers the resolved project, or an
  error naming what the tree is missing.
- `list_files()` answers every path, and `read_file(path)` one file's bytes, for a
  host that wants to persist or download what the session produced.
- `enable_fs_sink()` posts every later mutation to the page (below).

The tree lives as long as the tab. The page keeps the project beyond it: every
mutation is posted to the page, which writes it to
[a folder on disk](#a-folder-on-disk) or to [browser storage](#browser-storage),
depending on where the project came from.

### A folder on disk

Where the browser has the File System Access API (Chromium browsers), the page can
open a directory on the computer as the project instead of the bundled demo.

- The page reads the whole folder once (`.git`, `node_modules`, and `target`
  skipped) and seeds the tree with it, bytes as they are. A folder with no
  `jazyk.toml` at its top opens too, into the
  [setup wizard](./gui.md#the-setup-wizard), which writes the file the way any
  change lands (applied in writing mode, held in read-only mode).
- `enable_fs_sink()` turns on the write-through. From then on every `write`,
  `remove_file`, `remove_dir`, and `remove_dir_all` the backend makes is also posted
  to the page with the path and the bytes, and the page applies it to the folder
  through the directory handle. Seeding does not post: those bytes came from the
  folder.
- The page applies mutations one after another in the order they arrived, so a
  write and the remove that follows it never race. A mutation that fails to land
  is kept in browser storage, never dropped. See
  [when a folder cannot be written](#when-a-folder-cannot-be-written).
- Reads never touch the disk again. An edit made outside the tab is not seen until
  the folder is opened again. Nothing watches the folder.

A folder opens in one of two modes, writing unless `read-only` is chosen on
[Home](./gui.md#home), switchable from the [project menu](./gui.md#the-project-menu)
at any time. The mode is the page's: the worker posts every mutation either way.

- Writing: every mutation is applied to the folder as it arrives, and the
  [editor](./gui.md#editor) saves on its own.
- Read-only: the page asks the browser for read access only, and holds mutations
  instead of applying them: the latest one per path, in the order the paths were last
  touched, so a remove and a later write under the same directory replay in the
  order they happened. Documents, a build's shards, and the journal all stay in the
  tab. The top bar shows how many paths are held and offers to write them; a save in
  the editor asks the same. Writing asks the browser for write access, applies the
  held mutations in order, and switches to writing mode. A grant the browser
  refuses leaves the folder read-only, and the top bar says so. The held mutations
  are also kept in [browser storage](#browser-storage), so a reload or a closed tab
  does not lose them. Closing the tab with mutations held still asks first.

The recent list on [Home](./gui.md#home) remembers each folder's mode, so a folder
reopens the way it was left.

### Browser storage

A project that is not a folder on disk lives in the browser's own storage, so it
survives a reload and a closed tab. Three things use it:

- A browser project: a project started from scratch on [Home](./gui.md#home), not tied
  to any folder. See [a project in the browser](#a-project-in-the-browser).
- A demo with edits. See [an edited demo](#an-edited-demo).
- The changes a folder has not taken yet: the ones read-only mode holds, and the
  ones a failed write left. See
  [when a folder cannot be written](#when-a-folder-cannot-be-written).

What is stored where:

- File contents go to IndexedDB, database `jazyk-projects`, object store `files`,
  one record per file, so a write stores one file and never the whole project.
  - `file:<project>:<path>`: the bytes of one file of a stored project. `<project>`
    is `browser:<uuid>` for a browser project and `demo:<id>` for an edited demo.
  - `kept:<folder>:<path>`: one change a folder has not taken yet, `{ seq, op,
    bytes, dir }`, where `op` is `write` or `remove` and `seq` orders the changes.
    `<folder>` is the folder's recent entry, `folder:<uuid>`.
- The small metadata stays in local storage under `jazyk.recent`, as before: each
  entry's id, kind, name, a folder's mode, whether a demo carries edits, and how many
  changes a folder has kept. A folder's directory handle stays in IndexedDB's
  default database under its entry id.

Writes are coalesced. The page collects the posted mutations and stores them a
moment after the last one (a third of a second, and at most two seconds after the
first), the latest per path, in the order the paths were last touched, in one
transaction. A build's burst of shard and journal writes is one transaction, not
hundreds. Leaving the page (hiding it or closing it) stores what is pending at once.

Size and quota:

- A browser sets the quota per site, usually a share of the free disk. A browser
  project is a few kilobytes of documents plus its `.jazyk`. An edited demo is
  about the size of its fixture, up to about 1.3MB.
- The page asks the browser to keep the site's storage persistent
  (`navigator.storage.persist()`) when it first stores a project, so the browser
  does not evict it under storage pressure. A browser may refuse; the project is
  still stored.
- A write the browser refuses (the quota is full, or storage is unavailable) is
  not swallowed. The top bar says the project could not be saved in this browser and
  how many changes are in this tab only, and offers to retry and to download the
  project. The changes stay queued: every later store tries them again, in order.
- Browser storage is not a backup. Clearing the site's data deletes every browser
  project and every demo's edits. Save a project to a folder, or download it, to keep
  a copy.
- A browser with no IndexedDB (some private windows) stores nothing: a demo's edits
  stay in the tab, as the descriptor's `persistence: memory` says, and a new project
  is not offered.

### A project in the browser

`New project` on [Home](./gui.md#home) asks for a name and creates a minimal project
in browser storage, then opens it:

- `jazyk.toml`: the docs glob `docs/**/*.md` and the root `docs/README.md`.
- `docs/README.md`: a heading with the project's name and one line saying what to
  write.

E.g.:

```markdown
# Garden planner

Describe what this project is in plain sentences: what it is for, who uses it, and what it must do.
```

The recent list shows it as a browser project that lives in this browser only.
Reopening it from `Recent` reads its files from storage and seeds the tree with them.
It works in every browser, since it needs no folder API. Removing its entry from the
list deletes it, after asking.

A browser project leaves the browser two ways, both offered in the
[project menu](./gui.md#the-project-menu):

- `Save to a folder…` (where the File System Access API exists): the page asks for a
  folder, asks again if the folder is not empty, and writes every file of the tree
  into it. From then on the project is that folder in writing mode: the recent entry
  becomes the folder's, and the copy in browser storage is deleted once every file
  landed. A write that fails stops the save and says why: the project stays in the
  browser, and nothing kept there is deleted. Edits made while the files are written
  follow them into the folder.
- `Download`: the whole tree as one `.zip` named after the project, the same layout a
  folder would hold. It works in every browser.

A demo offers the same two.

### An edited demo

An unedited demo is fetched fresh every time it opens. The first mutation the backend
posts for a demo (a save in the editor, a chat edit, a build) turns it into a stored
copy under `demo:<id>`: the fixture's files as they were seeded, then every mutation
since. Its recent entry says it carries your edits.

- Reopening it from `Recent`, or picking the same demo again, opens the edited copy,
  not the fixture.
- `Reset to the original`, on its recent row and in the
  [project menu](./gui.md#the-project-menu), deletes the copy after asking. A demo
  that is open reopens from the fixture.
- Removing its entry from the list deletes the copy too, after asking.

The stored copy is a snapshot of the fixture it started from. A newer fixture in a
later release does not reach it until it is reset.

### When a folder cannot be written

A folder write can fail: the permission lapsed, the folder was moved or deleted, the
disk refused. The change is not lost:

- The failed mutation is kept in browser storage under the folder, and so is every
  mutation after it, so the folder never takes a later change before an earlier one.
  The tab keeps working on the tree as before.
- The top bar says it plainly: `Could not write to <folder>: N changes kept in this
  browser`, with `Retry`. Retry asks the browser for write access if it needs to,
  writes the kept changes in order, and stops at the first that fails again; the rest
  stay kept. While it writes, the top bar says `Writing N changes to <folder>…`.
- The recent row says how many changes the folder has kept.

Read-only mode's held changes are the same kept changes. Reopening a folder that has
kept changes, after a reload or on another day, reads the folder, lays the kept
changes over it in order, and opens that, so the tab shows the work as it was left.
The top bar offers to write them (`Write to folder`) or to discard them (`Discard`,
after asking, reopens the folder as it is on disk). In writing mode, while changes are
kept, new mutations queue behind them rather than landing first.

So a build in a tab lands in the same `.jazyk/` the native binary would write,
with the same shards and the same journal, and the two can be used on the same
project in turn.

## The product

A project that carries its product carries it into the tab: the
[deliverable](../consumers/gen.md#the-deliverable) folder, the
[ledger](../consumers/gen.md#the-ledger) under `.jazyk/gen/`, the criteria files,
and the generated pages under `.jazyk/docsgen/` are files in the tree like any
other. Every read of the product is served from them, through the same
`crate::webapi` functions the server answers with, so the [Product](./gui.md#product)
lens shows in a tab what it shows on a machine:

- `GET /api/deliverable`: the product files with the entities, requirements, and tests
  the ledger binds to each, and the recorded medium and build (the Files tree, the
  product form).
- `GET /api/deliverable/file`: one file's text with its resolved
  [sites](../consumers/gen.md#traceability), the code lenses of the product file
  viewer. `GET /api/deliverable/baseline` answers the file as it stood before the
  last generation, when the project carries that snapshot.
- `GET /api/verify/matrix`: the trace matrix, every requirement with its
  [derived status](../consumers/gen.md#status-is-derived-never-stored), and the Tests
  page built from it. The status is derived from the ledger and the files in the
  tree, the same way it is derived from the files on disk. `?format=csv` and
  `?format=md` answer the export as text, and the page saves it as a file.
- `GET /api/verify/pending`, `GET /api/gen/pending`, `GET /api/gen/package/{entity}`:
  what verification and generation owe, and the package a generation session would
  receive.
- The [unclaimed report](../consumers/bind.md#the-unclaimed-report) (the Not
  described page), through `GET /api/workers`.
- `GET /api/docsgen/{slug}`, `GET /api/out/list`, and `GET /api/out/file`: the
  generated pages and any other file under the out directory. The tab's envelope
  carries JSON, so `/api/out/file` answers a text file's text and refuses a binary
  file (a `.png`) with `501`.

Every path is confined the way the server confines it: a path that leaves the
deliverable or the out directory is `400`, a missing file is `404`.

What the product needs a shell for does not run: see
[what a tab does not do](#what-a-tab-does-not-do).

## Sessions

Natively a session is an [ACP worker session](./acp.md#worker-sessions): jazyk
spawns an agent, injects a `jazyk mcp` serving scoped to the batch, and the agent
talks to that serving over a transport. In a tab there is nothing to spawn and
nothing to connect over, so the same three pieces sit in one module and call each
other directly.

`crate::runner::SessionRunner` is the contract the reconciler and the ledger workers
speak: `agent_name`, `set_build_token`, `run_item`, `ask`, `ask_traced`. Two
implementations satisfy it.

- `crate::acp::runner::AcpRunner`, natively.
- `crate::wasm::runner::InPageRunner`, in a tab. It builds an `McpServer` in compile
  mode, lists its tools, claims the batch with `begin_goals` (whose reply carries the
  assembled session prompt), and drives `crate::agent_loop::run_loop` with a dispatch
  closure that calls `tools/call` on that serving.

The serving is the same one a native session gets, in the same mode, so the
[gates](../compiler/graph.md#validation-gates), the staged changeset, and the atomic commit are
the ones every other front door uses. What landed is read from the
[journal](../compiler/graph.md#journal) through `crate::runner::journal_diff`, never
from the model's word. A batch the model leaves open never commits: the changeset is
abandoned and the goals return to open.

`crate::agent_loop` is the generic loop the embedded agent already used: send the
history and the tools, dispatch the calls the model makes, repeat until it stops
calling tools. It carries the codec machinery (native tool calls, the text fallback,
the first-round probe) and knows nothing about jazyk, which is why it serves both.

## The model

A session is synchronous all the way down: it calls the model and waits for the reply
before it decides what to stage. Every model API in a browser is a promise, and a
promise cannot be awaited from synchronous Rust. The two are joined the only way that
composes without turning the reconciler inside out:

- The backend runs in a Web Worker.
- The page owns the model and stays on the main thread.
- A call writes the request out and blocks the worker on `Atomics.wait`.
- The page runs the model, writes the reply into a `SharedArrayBuffer`, and wakes the
  worker with `Atomics.notify`.

`crate::llm` is unchanged above the transport: it assembles the same
OpenAI-shaped request, and `Transported` is the one seam. Natively the transport is
HTTP through `ureq`; in a tab it is `crate::wasm::llm_bridge`, whose `chatSync` does
the blocking handshake above. Streaming has no transport to stream over, so the page
answers once.

`SharedArrayBuffer` requires a [cross-origin isolated](#serving) page. Without it the
backend still serves every read; only a build or a chat turn is refused, with the
reason. The same
holds without WebGPU: there is no model to call, and the page says so instead of
hanging.

The backend speaks the text codec in a tab, set once at boot (the same effect as
`JAZYK_CODEC=text`). WebLLM's native tool calling is whitelisted to a few 8B builds
and refuses a custom system prompt beside it, so the first-round probe would fail
every session before falling back. The text codec renders the compile serving's tool
schemas into the prompt, about 19k tokens before the model says a word, so the
engine's context window is set to 32k. The window is paid for in KV cache: for a 3B
model about 3.5GB beside 2GB of weights, which a laptop with unified memory carries.

The model the page loads is a small instruct model, so a first visit downloads
gigabytes rather than tens of gigabytes, and the weights cache in the browser. A
model this size is well under what the [benchmark](../benchmark/benchmark.md) asks
for: the harness holds (gates bounce bad calls, junk never lands), but the judgment
the sessions produce is poor, and a turn takes a minute or more. The demo shows the
machinery, not the quality.

### Model sources

The page's model comes from one of three sources, chosen in the
[action button](./gui.md#the-action-button)'s popover, in Settings, and in the chat
pane while no model is ready. Builds and [chat](#chat) use
the same one. The choice, and an endpoint's URL, model name, and key, are kept in this
browser's local storage under `jazyk.modelSource`. Nothing is saved on a jazyk server;
an endpoint's key goes only to that endpoint.

- Downloaded (the default): an instruct model from Hugging Face run on WebGPU by WebLLM,
  loaded on request and cached by the browser (above). The model is a choice: the
  chooser lists WebLLM's prebuilt instruct models (Llama 3.1 and 3.2, Qwen 2.5,
  Hermes 2 and 3, Mistral, Phi, Gemma, SmolLM, in their 16-bit-activation builds)
  with the memory each needs, from a few hundred megabytes to about six gigabytes,
  and the default is Hermes-3-Llama-3.2-3B (2.2 GB). A bigger model is a better
  build and a longer download; the 7B and 8B tier needs a machine with several
  gigabytes to spare beside the weights, since the 32k window's KV cache comes on
  top. The choice is kept with the source under `jazyk.modelSource`, and a model
  chosen after another was loaded loads on request like the first one.
  - Each model carries a tier, from its parameter count, so the list says what a
    model is for. The rule is the count, not a name, so a model the table gains
    later sorts itself.
    - `build`: 7B and up. Builds well, and chats.
    - `chat`: 3B. Chats and answers questions, and builds with rough results.
    - `reader`: under 3B. Answers questions over the graph's read tools only, and
      does not build.
  - The list marks the models already in the browser's cache (`downloaded`, from
    WebLLM's `hasModelInCache`), so what is one click away is visible, and it is
    sorted strongest first.
  - On arrival the page picks the strongest downloaded model and loads it, with no
    source chosen yet or with the downloaded source chosen; a page with nothing
    downloaded offers the default and waits to be asked. A remembered choice
    ([the demo](#the-demo)) is kept when it is still downloaded.
  - A build runs on a `build` or a `chat` model. A `chat` model carries an info
    tooltip in the chooser: the model is small, a build runs, and its results are
    rough; a 7B or larger model, or an endpoint, builds better.
  - With no model or a `reader` model loaded, the Build button opens the chooser
    instead of running, and the tooltip says a build needs a model of 3B or more,
    or an endpoint, since a session's prompt and the judgment it asks for are
    beyond a model that size and the build would only park. Chat runs on any tier.
- Built into the browser: Chrome's on-device model (Gemini Nano) through the Prompt
  API (`LanguageModel`). Offered only where the browser has it. "Built in" means the
  browser owns and runs it, not that it is already on the disk: Chrome fetches the
  weights, a few gigabytes, the first time any page asks, and the page says so
  (`Chrome is downloading its built-in model for the first time`) with the progress
  the browser reports; later loads are instant. Its context window is small, so it
  counts as a `reader` model (the tiers above): chat and questions over the read
  tools, and a build asks for a downloaded model of 3B or more, or an endpoint. A prompt it
  still cannot hold fails with the model's own words ("The input is too large.") and
  the remedy beside them (a downloaded model or an endpoint).
- Your endpoint: any OpenAI-compatible chat completions URL (a local server, LocalRouter,
  a provider), with a model name and an optional key. The page calls it directly, so
  the endpoint must allow this page's origin (CORS). Ready at once, nothing to load.

Switching sources drops the loaded model; a session in flight finishes on the old one.
A source that cannot run in this browser is shown with the reason instead of being
offered.

## The trace

The native server streams [trace events](../compiler/sessions.md#trace-events) over
SSE. The browser backend has no stream and no moment to poll: a build runs
synchronously inside the worker, and the worker answers nothing until it returns.
So each event is posted to the page the instant the compiler emits it, from inside
the running build, and the page pushes it through the same path a streamed event
takes. The workbench renders a browser build exactly as it renders a server one,
and the page's console carries the same events one line each.

The model host logs one line per call (messages, characters, tools) and one per
reply (tokens in and out, the finish reason, the tools called, the first words),
and a progress line every hundred chunks while a slow model works, because the
backend is blocked on the answer and can say nothing itself.

## Chat

The [chat column](./gui.md#chat) works in a tab, on the model the page chose. There
is no agent process to spawn, so a conversation runs the way a goal batch does in
[sessions](#sessions): the generic loop, a serving, and the page's model, in one
module.

- `POST /api/chat/sessions` opens a conversation in the worker. It holds an
  `McpServer` in `chat` mode (ephemeral, like the serving a native chat session
  gets), the running history, and the numbered updates. The serving lives as long as
  the conversation, so the loaded set carries from one prompt to the next. A
  `reader`-tier model ([model sources](#model-sources)) gets the `graph` serving
  instead: the read tools alone (`search`, `load`, `expand`, `read_section`,
  `get_entity`, `get_view`, `diagnostics`) and a short instruction block, so the
  prompt fits a small window and the model answers questions about the project
  without being offered edits it cannot make well. The page passes the tier when it
  opens the conversation (`{"tier": "reader"}` in the body).
- `POST /api/chat/sessions/{id}/prompt` runs one turn: `crate::agent_loop::run_loop`
  with the serving's tools and a dispatch closure that calls `tools/call` on it,
  against the page's model through [the bridge](#the-model). The first turn opens
  with the serving's instructions, so the model knows which project it is in and
  which tools move the prose. A prompt sent while a turn runs is answered `409` by
  the page, since the worker is inside the turn and runs one at a time.
- The loop runs as a conversation: a prose reply after tool calls is the answer and
  ends the turn. The finish nudge a worker session gets
  ([the embedded agent](./acp.md#the-embedded-agent)) would hold the answer back and
  ask for a one-line summary in its place. The other nudges (an announced action, an
  empty reply, a cut call) still apply.
- The request answers when the turn ends. Until then the worker posts each event to
  the page as it happens, in the shape the native [event stream](./gui.md#events)
  carries: `chat.update` (a message, a tool call, its result, the end of the turn),
  `chat.sessions` (a conversation starts or stops running), and `store.generation`
  when a tool call commits. The page hands them to the handler the stream drives, so
  the pane renders a tab's conversation exactly as it renders a server's.
- `GET /api/chat/sessions` and `GET /api/chat/sessions/{id}` answer the native
  shapes.
- `POST /api/chat/sessions/{id}/cancel` is answered by the page, since the worker is
  inside the turn. The page sets a cancel flag in the shared control buffer and fails
  the model call in flight, so the turn ends at once as `cancelled`. A reply that
  arrives after the cancel is dropped.

What the serving does in a tab:

- The same [gates](../compiler/graph.md#validation-gates), the same staged
  changesets, the same [dual writes](./acp.md#dual-write-tools). A chat edit to a
  document lands in the tree. With a [folder](#a-folder-on-disk) open it follows the
  folder's mode: applied in writing mode, held in read-only mode. In a stored project
  it is stored like any edit ([browser storage](#browser-storage)).
- The binding, generation, and verification lifecycles are not served: each needs a
  shell. The compilation lifecycle stays. Fewer tools is also a shorter prompt, since
  the text codec writes every tool's schema into it.
- No [slash commands](./acp.md#slash-commands): the catalog lives in `acp`, which is
  gated off the wasm build. The list answers empty, and a prompt that starts with
  `/` reaches the model as text.
- The chat does not start a build. The tab's chat instructions, the `graph`
  serving's short block included, say that builds run from the Build button at the
  top of the page, so a model asked to build points there instead of asking for
  prose to build from.
- No permission requests: the loop never asks.
- A conversation lives in the worker for the life of the tab. Nothing is written to
  the [session store](./acp.md#session-store), so a reload starts with none.

The model:

- The descriptor says `agent: embedded` in a tab whether or not a model is ready;
  `model` says whether it is. While it is not `in-page`, the pane says the chat needs
  the model and shows the [model sources](#model-sources) chooser with its load
  button, the one the action button's popover shows.
- A turn the model fails ends with the model's error in the transcript, never a hang.
  A small context window is the usual cause: the conversation, the instructions, and
  the tool schemas travel on every call, and on the browser's built-in model they may
  not fit at all. The error line then says the conversation does not fit the model's
  context and names the remedy: a new conversation, or a source with a larger window.
  A rate limit or a spent quota (`429`, `rate limit`, `quota`) is told apart from that:
  its line says to wait or choose another source, since a new conversation would not
  help.

## A measured run

On an Apple laptop with Hermes-3-Llama-3.2-3B on WebGPU, one `reconcile-section`
session over the demo project: 15451 tokens in, prefill about 85 seconds, decode
about 4 tokens a second, 1023 tokens out and cut by the cap. The model answered
with the text codec's JSON object rather than a tool call, the session ended with
nothing committed, and the reconciler retried the batch, then parked the goal. The
gates held and nothing landed. A model of this size shows the machinery; the
[benchmark](../benchmark/benchmark.md) says what a model needs to do the work.

## Diagrams

Rendering stays native. `plantuml-little` links Graphviz as a prebuilt C archive, and
there is no wasm build of it, so `render_svg` and `render_png` answer an error in a
tab. Everything above them still runs: the emitters, the
[lifting and collapse](../compiler/diagrams.md#lifting-and-collapse), the over-limit
handling. Every view still emits its `.puml` on commit, and the `.svg` beside it is
whatever the project was seeded with.

This is why the demo ships a project whose diagrams are already rendered: the
`.svg` files travel in the seed. A read of a view (`GET /api/views/{id}`) that cannot
render serves the `.svg` on disk when the view's emission equals the `.puml` beside
it, so the picture is exactly the view's. A view that changed since the seed, or one
drawn at another level of detail, has no picture in a tab, and its page says why.

## The demo

The page mounts at once against a backend with no project open, which answers the
[capability descriptor](./gui.md#backends-and-capabilities) itself (`backend:
demo`, `projects: bundled`, `events: pushed`, `lsp: false`, `agent: embedded`,
`rendering: none`, `persistence: browser`) and
refuses every other route with `501` until a project is open. Home renders the
picker from that descriptor and dispatches `jazyk:open-demo`, `jazyk:pick-folder`,
`jazyk:new-project`, `jazyk:connect`, or `jazyk:open-recent`; the entry listens,
seeds the backend with the bundled project, the [folder](#a-folder-on-disk) picked,
or the [stored project](#browser-storage), or connects to the
[local server](./gui.md#connecting-to-a-local-jazyk), and moves to the documents.
Opening a folder flips the descriptor to `backend: wasm`, `persistence: folder`;
opening a [browser project](#a-project-in-the-browser) flips it to `backend: wasm`,
`persistence: browser`; connecting swaps the backend for the server's own
descriptor. The open project is remembered and reopened on a reload
([the open project](./gui.md#the-open-project)).

Jobs in a tab: a build runs inside the worker, synchronously, so the worker itself
can emit no lifecycle. The page keeps the job list and emits the lifecycle around
the call: `POST /api/jobs` mints the job id and posts `job.queued` and
`job.started`, every trace event the worker posts while the call runs carries that
id as `job.trace`, and the return posts `job.finished` with the verdict.
`GET /api/jobs` answers the page's list, newest first, kept for the life of the tab;
opening another project empties it, since its threads belong to the project that ran
them. So the [action button](./gui.md#the-action-button) shows the run's progress and the
[chat column](./gui.md#chat) opens its build thread in a tab exactly as on a server.

The model loads only when asked, because every read works without one and only a
build or a [chat](#chat) needs it, and a first visit would otherwise download
gigabytes to read a document. The descriptor says `model: loadable` while the chosen
[source](#model-sources) still has to load, and the action button's popover and the
chat pane offer `Load the model` with its download size; the load shows its
progress there, and the weights cache in the browser, so a later visit loads from
the cache. A source loaded once in this browser loads again on the next visit
without asking (`jazyk.modelLoaded` in local storage remembers which), so a reload
lands in a project with its model ready; choosing another source in the chooser
stops that until the new one is loaded once. `jobs` lists `compile` once the model
is ready and its tier is `build` or `chat`. The model's code (WebLLM) is fetched
with the model, not with the page.

Every example Home lists is a fixture: the project's documents, its `jazyk.toml`,
its product folder when it has one, and its whole `.jazyk` as last built (the
shards, the journal, the coverage, the rendered diagrams, the ledger), packed by
`app/scripts/pack-demo.mjs` into one JSON per example under `app/public/demos/`. Traces, session transcripts, the per-batch snapshots kept
for replays, leases, worker registrations, sockets, dotfiles (`.jazyk` itself is the
one dotfolder carried, since it is the out directory), `__pycache__`, an
archived out directory, and any file that is not UTF-8 text (a built deck, a compiled
module) are left out: a stale worker registration would be reaped on the first
read, and that removal would count as an edit, and a binary file would only arrive
as replacement characters. A packed file unpacks byte for byte (a byte order mark
stays). The fixtures are committed, since the
examples' out directories are not, and `npm run pack-demos` repacks all of them
after an example is rebuilt. The page fetches a fixture when its demo is picked
rather than carrying them all.

So a visitor lands on a populated graph rather than an empty one: the entities, the
requirements, the views, the diagrams, and the verdict the last build left. Editing
a document dirties its sections, which opens goals, and a build runs them through
the model in the page. The first edit keeps the demo in
[browser storage](#an-edited-demo), so it survives a reload until it is reset.

### Keeping the demos current

The demos show the work as it stands, so they are rebuilt as the compiler changes: the
graph, the criteria, the diagrams, and the product each example generates (its code or
output and its tests), with the tests run and their results in the ledger.

- `app/scripts/refresh-demos.sh` rebuilds every example in its own directory and
  repacks the fixtures: `jazyk compile` until the graph converges, `jazyk gen` (bind,
  generate, and verify the product), `jazyk test` (run every test), every diagram
  redrawn by the current renderer, then `npm run pack-demos` and the browser tests. It
  ends with a table per example: the verdict, the open and blocked goals, the tests
  passing out of the ledger's rows, and the tokens the refresh billed. The model steps
  cost money, so the script runs only when a person runs it, with the model named
  (`--model gpt-5.5`); `--only <id>` rebuilds one example; `--free` skips every model
  step and only redraws and repacks. An example whose model steps hit a rate limit (an
  `HTTP 429`, or a rate, usage, or quota limit named in a trace the run wrote) is not
  packed: its fixture stays as committed,
  the table says `rate limited`, and the script exits non-zero, so a half-finished
  graph never reaches the demos.
- The deploy redraws the committed fixtures' diagrams with the renderer it just built
  (`app/scripts/rerender-fixtures.sh`), so a renderer change reaches jazyk.dev without a
  rebuild. Nothing else of a fixture changes at deploy.
- CI reports a demo that fell behind as a warning on the run, never a failure, so a
  compiler change is never held up by a demo. `app/scripts/check-demos.sh` unpacks
  every fixture into a temporary directory and warns about:
  - a fixture whose store version is not the binary's. `jazyk status` reads such a
    store as empty and only a build archives it, so the script compares the packed
    `version` with the one `status` reports.
  - a fixture with open goals the current compiler would derive.
  - a fixture whose ledger has failing, stale, or missing tests
    (`app/scripts/ledger-count.py`).
  - a fixture whose diagrams are not the current renderer's output. `jazyk docsgen`
    on the unpacked fixture draws every view whose committed `.puml` is not what the
    emitter writes or whose `.svg` is missing, and its summary line counts them; one
    drawn is drift. This warning names `rerender-fixtures.sh`, since a redraw is free.
    The others name `refresh-demos.sh`.
  A second CI step proves the check sees a stale demo: it runs the script on a copy of
  one fixture with its store version set back and on a copy with a diagram's source
  changed, and fails unless each warns.

## Building

`cd bootstrap/app && npm run build` runs the whole pipeline:

- `scripts/build-wasm.sh`: `cargo build --target wasm32-unknown-unknown --lib
  --release`, then `wasm-bindgen --target web`, then a stable re-export for the model
  bridge (wasm-bindgen names its snippet directory after a content hash).
- `vite build`, which builds the one page into `dist/` beside the fixtures from
  `public/demos/`, the web manifest, and the service worker with its precache
  manifest ([offline](#offline)). `npm run preview` serves it with the isolation
  headers. `dist/` is build output, never committed: the binary embeds nothing.

`npm run test:e2e` runs the editor's browser tests (`app/e2e/`, Playwright) against
the built `dist/` served with the isolation headers (`PW_PORT` picks the port, so
two checkouts run them side by side; `PW_CHANNEL=chrome` runs them in the installed
Chrome). They drive the page the way a visitor does: every route but Home redirects until a project is open, each
demo opens with only its own documents, the Model pages read by name with no ids,
a requirement keeps the Requirements list, a diagram shows its picture, the
Product pages list a demo's product files and open one with its text, the trace
matrix renders its rows, a demo autosaves, a read-only folder holds its writes
until asked and a writing one autosaves (a folder in the browser's private file
system stands in for a picked one), the recent list remembers and forgets, a new project and an edited demo
reopen with their edits after a reload and the demo resets to its original, a
folder whose writes fail keeps them and Retry lands them, a read-only folder's held
changes survive a reload, a phone-width screen gives the editor
the width, nothing downloads the model on arrival, the chat pane offers the model
chooser while no model is ready, a chat on an endpoint source (a mocked one)
calls a read tool and streams its answer into the pane, the endpoint call carries
the key only when one is set, a second prompt during a turn is refused, switching
demos empties the conversations, a stubbed built-in model answers a chat, a
download is a `.zip` of the project, a refused write grant is said in the top bar,
a requirement page reads its criteria in words and its ids only in Developer mode,
every diagram of every demo has its picture, a phone-width screen shows the
chat again once it is wide, a reload lands in the open project at the same route,
a build in a tab shows its progress on the action button and opens its build thread
in the chat, the chat's prompt box takes a first question with no conversation
open, the action list offers only what the backend runs, the project menu says
where changes go and downloads the project, the page loads and opens a demo it has
seen with the network off, and the connect route reaches a mocked local server and
lists it in Recent. The deploy workflow runs them after the build and
publishes only when they pass.

- `npm run test:unit` runs the unit tests of the pure logic under `app/src/lib/`
  (Vitest, one `<module>.test.ts` beside each module, jsdom with a fake IndexedDB):
  the recent list, the coalescing store writer, the folder mirror, the model
  sources, and the presentation words. The same run covers the demo fixture scripts
  under `app/scripts/` (one `<script>.test.mjs` beside each, in node): `pack-demo.mjs`
  and `unpack-demo.mjs` round-trip a project tree, minus the skipped entries and the
  binary files, byte for byte; `ledger-count.py` counts a ledger with a quoted
  verdict, a criterion's verdict beside its row's, a verdict line inside a block
  scalar `evidence`, a failing row, and an unverified row. They need no build and no
  browser, so CI runs them on every change to `bootstrap/`.

The page's first script carries the app alone. What only some visits use loads when
it is first used: the model's code when the model loads, and the graph layout
library when the Developer [map](./gui.md#map) opens.

The wasm target lives in the rustup toolchain. A `cargo` earlier on `PATH` from
another installation (a Homebrew rustc has no wasm std) is the usual failure;
`build-wasm.sh` asks rustup which cargo owns the target and puts its `bin` first so
`cargo` and `rustc` stay matched.

Requirements: the `wasm32-unknown-unknown` target (`rustup target add
wasm32-unknown-unknown`) and `wasm-bindgen-cli` at the version of the `wasm-bindgen`
crate (`cargo install wasm-bindgen-cli --locked`).

## Serving

The demo page must be cross-origin isolated, or `SharedArrayBuffer` is not available
and no build can run. That means two headers on the document:

- `Cross-Origin-Opener-Policy: same-origin`
- `Cross-Origin-Embedder-Policy: require-corp`

The Vite dev server and `vite preview` set both. [jazyk.dev](../site.md#the-editor)
serves the page from S3 through CloudFront, which sets them with a response headers
policy; nothing client-side is needed. A response the service worker serves from
its cache keeps the headers it was cached with, so an offline load is isolated too.

## Offline

The page is installable and works without a network once it has loaded once:

- A service worker (Workbox through `vite-plugin-pwa`) precaches the app shell:
  `index.html`, the scripts and styles, the wasm module and its bindings, the
  fonts, the icons, and the web manifest. It serves them cache-first, and updates
  the cache in the background when a new build is deployed; the next load runs the
  new version.
- A demo's fixture (`demos/<id>.json`) is cached when it is first fetched and
  served from the cache after, refreshed in the background when the network is
  there, so a demo opened once opens offline. The model's weights are the model
  library's own cache, filled on the first load.
- Browser projects, folders, and the edits kept for a demo are in the browser's
  storage and never needed the network. A connection to a local server is on the
  loopback address and needs no network either.
- What needs the network: the first visit, a demo never opened, a model never
  loaded, and the endpoint model source.
- A build a person is offline for is not the deploy's concern: `index.html` and the
  fixtures are served with no cache at the edge, the hashed assets with a week, and
  the worker's precache manifest names each asset by its hash, so an update is one
  new manifest and the assets it names.

## What a tab does not do

- No [generation](../consumers/gen.md), [binding](../consumers/bind.md),
  verification, or [decompilation](../consumers/decompile.md) runs: each shells out to the deliverable's build and test commands, or
  runs a session with command tools, and `crate::sys::run_shell` has no shell. The
  jobs answer `501`, and the pages hide the actions that would start them (build,
  check, audit, document), as they hide any action a missing
  [capability](./gui.md#backends-and-capabilities) asks for. The product the project
  carries still reads: see [the product](#the-product).
- No [LSP](./lsp.md), no [ACP](./acp.md), no external [executors](./acp.md#agents):
  all three need a process or a socket. [Chat](#chat) runs the loop in the worker
  instead of an agent.
- No binding, generation, or verification in chat, and no slash commands. See
  [chat](#chat).
- No rendering to `.svg` or `.png`. See [diagrams](#diagrams).
- No file watching. The only way onto the machine is a folder the person opened or
  saved to, or a download.
- No [benchmark](../benchmark/benchmark.md).
