# Browser Backend

A project opens all [document sets](../compiler/document-sets.md). Graph operations address one set
explicitly. Each set owns `.jazyk/<set-id>/`; project file operations share the
physical files and report every membership. The document-set contract defines
configuration, aggregate reads, management commands, and grouped compilation.

The compiler library builds for `wasm32-unknown-unknown`. It uses the same graph,
reconciler, board, goals, tools, and [MCP serving](./mcp.md) as the native engine.
The portable engine and its JavaScript bridge source remain in this repository.
The complete jazyk.dev application, persistence adapters, model-source chooser,
fixtures, and PWA build live in the separate
[Jazyk Cloud browser application](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/browser.md).

The engine has no dependency on that private application. A compatible host supplies
files, persistence, events, and model responses. The [protocol version](./protocol.md)
is independent of compiler and application releases.

## One API, two transports

The [GUI](./gui.md) API is one implementation with two ways in.

- `crate::webapi` holds the values: `status_value`, `board_value`, `preview_value`,
  `explain_value`, `ripple_value`, `views_value`, `view_value`, `tree_value`,
  `card_value`, `page_value`, and the [product](#the-product)'s
  `deliverable_value`, `deliverable_file`, `deliverable_baseline`, `matrix_value`,
  `matrix_export`, `gen_pending_value`, `gen_package_value`, `verify_pending_value`,
  `docsgen_value`, `out_list`, and `out_file`, and [the Inbox](#the-inbox)'s
  `inbox_value`, `questions_value`, `triage_value`, and `answer_value`, and the
  [workflow modes](#workflow-modes-in-a-tab)' `watch_value` and `watch_write`. Each takes a
  [store](../compiler/graph.md) (or a project and an out directory) and answers a
  JSON value, or a refusal with its status and reason. Nothing in it knows about a
  transport.
- `crate::gui` is the native transport: axum handlers, each a thin wrapper that loads
  the store off the async runtime and calls one of those functions.
- `crate::webapi::dispatch` is the browser transport: one `request(method, path,
  body)` that routes the same method and the same path to the same functions and
  answers the same JSON.

The browser request envelope is `{ status, body }`. Hosts translate a native HTTP
response or a WASM module response into that shape. Engine endpoints do not depend
on the application transport adapter.

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
- `GET /api/product/overview`: the product as the [overview](./gui.md#overview)
  shows it, a written product's parts in reading order, a built product's artifacts,
  or a program's README and run commands.
- `GET /api/deliverable/raw`: one product file's bytes, a built artifact the overview
  renders or downloads. The tab's envelope carries JSON, so the body is
  `{ "path", "size", "contentType", "base64" }`, the bytes base64-encoded, and the
  page decodes it into the same bytes the server sends.
- `GET /api/project/raw`: one project file's bytes, text or not, for a preview, in
  the same `{ "path", "size", "contentType", "base64" }` body as
  `GET /api/deliverable/raw`.
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

## The Inbox

The [Inbox](./gui.md#inbox) works in a tab. Its reads and writes are the server's
own, from `crate::webapi::inbox`:

- `GET /api/inbox`: the rollup. The top bar's count, the navigator's section
  counts, and the Inbox's lists all read it.
- `GET /api/questions`: the standing questions.
- `POST /api/diagnostics/{id}/triage`: Seen, Won't fix, Hide, and Undo, committed
  through the store as a `triage` entry.
- `POST /api/questions/{id}/answer`: an answer, through `crate::answer`, the engine
  every frontend calls ([answers](../compiler/model/diagnostic.md#answers)).

What an answer does in a tab:

- An `edit` option applies and resolves in the reply, the way it does on a machine:
  the dual write lands in the tree and the question leaves the list. A ratification
  proposal's `Accept` and `Retract` are the same.
- An `answer` option or a freeform reply records the text with
  `answer.status: handling`. A tab runs nothing in the background, so no
  [answer session](./acp.md#answer-sessions) starts on the reply. The reply says
  so. The recorded answer is build work: the card says `Queued for the next build`
  with a `Build now` action, and the [action button](./gui.md#the-action-button)
  counts it (`Build · 1 answer` when nothing else waits). With `compile: auto` the
  page starts that build itself ([workflow modes in a tab](#workflow-modes-in-a-tab)).
- The next build runs it. Before its first goal batch, a build in a tab runs one
  answer session per `handling` answer, on the page's model, the way
  [sessions](#sessions) run: the `chat` serving, the prompt carrying the
  diagnostic, the question, the reply, and the subjects. The status moves to
  `handled` when the session ends, or `failed` with the error in `answer.error`.
  The build's reply lists them under `answers`, and each session's events reach the
  trace under the label `answer <diagnostic>`, in the build thread. The prose edits
  a session makes are dual writes: an open editor takes them in place and marks
  them "changed by your answer" ([editor](./gui.md#editor)).
- A build needs a model, so without one the answer waits with `handling`.
- A [chat](#chat) answers the same way a native chat session does: the agent
  records a reply relayed in conversation with `answer_diagnostic` and acts on it
  in the same turn ([questions in chat](./acp.md#questions-in-chat)).

## Workflow modes in a tab

The [workflow modes](./gui.md#workflow-modes) are control-plane state in a tab too:
`control.yaml` in the tab's out directory, read and written through
`crate::control`, which compiles for both targets. `GET /api/watch` and
`PUT /api/watch` answer from `crate::webapi::workers`, the server's own
`watch_value` and `watch_write`, and a mode set in the tab is kept with the project
like any other file.

- Settings shows the Build switch: Build on save or Build on request. A tab runs no
  generation, so it has no Generate switch.
- `compile: manual` (the default): a build runs on a click on the action button.
- `compile: auto`: the page starts a build a moment after a document save and after
  a recorded answer. Saves and answers in quick succession start one build. A
  trigger that arrives while a build runs waits, and the next build starts when
  that one ends. Without a model that builds nothing starts, and the work waits for
  the action button.
- An applied `edit` option starts nothing: its dual write leaves no section to
  reread.

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

The runner emits the session's [trace events](../compiler/sessions.md#trace-events)
the way the ACP runner's translator does: `sessionStart`, one `toolCall` per call
with its `toolResult` or `toolError`, a `section` event when an accepted call names
another section, the model's text and reasoning as `modelText`, and `sessionDone`
when the batch lands. A batch that does not land fails with the stop in words (the
model ended its turn with the batch still open, the round budget, the model's error).

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
- The JavaScript host owns the model and stays on the main thread.
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

## Building

Build the engine library from `bootstrap/`:

```sh
cargo build --target wasm32-unknown-unknown --lib --release --locked
```

The toolchain needs the `wasm32-unknown-unknown` target. A host generates JavaScript
bindings with a `wasm-bindgen-cli` version matching the crate's locked `wasm-bindgen`
version. The engine checkout includes embedded compiler payloads under `docs/`.
The application consumes a pinned engine revision and owns its bundling and output.
See the [application build](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/browser.md#building).

## Serving

The synchronous model bridge requires `SharedArrayBuffer`. Its host document must
be cross-origin isolated with these response headers:

- `Cross-Origin-Opener-Policy: same-origin`
- `Cross-Origin-Embedder-Policy: require-corp`

Without isolation the engine's reads remain usable; model calls report the missing
capability. Offline caching and deployment belong to the
[browser application](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/browser.md#serving).

## Browser application specification

These host and presentation sections moved to Jazyk Cloud. Existing anchors link
to their canonical application specifications.

- <a id="a-folder-on-disk"></a>[A folder on disk](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/browser.md#a-folder-on-disk).
- <a id="browser-storage"></a>[Browser storage](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/browser.md#browser-storage).
- <a id="a-project-in-the-browser"></a>[A project in the browser](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/browser.md#a-project-in-the-browser).
- <a id="an-edited-demo"></a>[An edited demo](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/browser.md#an-edited-demo).
- <a id="when-a-folder-cannot-be-written"></a>[When a folder cannot be written](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/browser.md#when-a-folder-cannot-be-written).
- <a id="model-sources"></a>[Model sources](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/browser.md#model-sources).
- <a id="the-demo"></a>[The demo](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/browser.md#the-demo).
- <a id="keeping-the-demos-current"></a>[Keeping the demos current](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/browser.md#keeping-the-demos-current).
- <a id="offline"></a>[Offline](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/browser.md#offline).

## Document-set chat actions

A project chat lists all document sets. A writer can preview and save their
selections and call `compile_document_sets`. Reader sessions keep read-only tools.
The compile action accepts explicit set IDs or all dirty sets.

## Stopping builds

The page sets the shared cancellation flag for the selected running job and wakes an
outstanding model request with cancellation. The compiler checks the flag before
another request or tool call. Late model replies are ignored. Completed session
commits remain; staged work and provisional display are discarded. The job ends
`cancelled`. An explicit Build resets the flag and derives remaining work from saved
documents. Stop suppresses automatic follow-up builds until an explicit Build.
