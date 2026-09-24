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
  `card_value`, `page_value`. Each takes a [store](../compiler/graph.md) (or a
  project and an out directory) and answers a JSON value. Nothing in it knows about a
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
- `list_files()` answers every path, for a host that wants to persist or download
  what the session produced.

The tree lives as long as the tab. In the demo's in-memory mode nothing is written
through to the machine and a reload starts from the seed again. With
[a folder on disk](#a-folder-on-disk) open, every mutation is mirrored into it.

### A folder on disk

Where the browser has the File System Access API (Chromium browsers), the page can
open a directory on the computer as the project instead of the bundled demo.

- The page reads the whole folder once (`.git`, `node_modules`, and `target`
  skipped) and seeds the tree with it, bytes as they are. The folder must hold a
  `jazyk.toml` at its top.
- `enable_fs_sink()` turns on the write-through. From then on every `write`,
  `remove_file`, `remove_dir`, and `remove_dir_all` the backend makes is also posted
  to the page with the path and the bytes, and the page applies it to the folder
  through the directory handle. Seeding does not post: those bytes came from the
  folder.
- The page applies mutations one after another in the order they arrived, so a
  write and the remove that follows it never race. A mutation that fails to land
  is logged; the tree in the worker is still the truth the compiler reads.
- Reads never touch the disk again. An edit made outside the tab is not seen until
  the folder is opened again. Nothing watches the folder.

So a build in a tab lands in the same `jazyk-out/` the native binary would write,
with the same shards and the same journal, and the two can be used on the same
project in turn.

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
[gates](../compiler/graph.md#gates), the staged changeset, and the atomic commit are
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
backend still serves every read; only a build is refused, with the reason. The same
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
`.svg` files travel in the seed.

## The demo

`bootstrap/app/demo.html` is the same app with a different entry. The app mounts at
once against a backend with no project open, which answers the
[capability descriptor](./gui.md#backends-and-capabilities) itself (`backend:
demo`, `projects: bundled`, `events: pushed`, `lsp: false`, `rendering: none`) and
refuses every other route with `501` until a project is open. Home renders the
picker from that descriptor and dispatches `jazyk:open-demo` or
`jazyk:pick-folder`; the entry listens, seeds the backend with the bundled project
or the [folder](#a-folder-on-disk) picked, and moves to the documents. Opening a
folder flips the descriptor to `backend: wasm`, `persistence: browser`. The model
loads behind all of it, because every read works without one and only a build
needs it; `jobs` lists `compile` once the model is ready.

Every example Home lists is a fixture: the project's documents, its `jazyk.toml`,
and its whole `jazyk-out` as last built (the shards, the journal, the coverage, the
rendered diagrams), packed by `app/scripts/pack-demo.mjs` into one JSON per example
under `app/public/demos/`. Traces, session transcripts, leases, sockets, dotfiles,
and an archived out directory are left out. The fixtures are committed, since the
examples' out directories are not, and `npm run pack-demos` repacks all of them
after an example is rebuilt. The page fetches a fixture when its demo is picked
rather than carrying them all.

So a visitor lands on a populated graph rather than an empty one: the entities, the
requirements, the views, the diagrams, and the verdict the last build left. Editing
a document dirties its sections, which opens goals, and a build runs them through
the model in the page.

## Building

`cd bootstrap/app && npm run build:demo` runs the whole pipeline:

- `scripts/build-wasm.sh`: `cargo build --target wasm32-unknown-unknown --lib
  --release`, then `wasm-bindgen --target web`, then a stable re-export for the model
  bridge (wasm-bindgen names its snippet directory after a content hash).
- `vite build` with `JAZYK_DEMO=1`, which builds `demo.html` into `dist-demo/` as
  `index.html` (the editor is the root of its site) beside the fixtures from
  `public/demos/`. `npm run preview:demo` serves it with the isolation headers.

The demo lands in its own directory so the dist the binary embeds never grows a 7MB
module. `npm run build` is unchanged and still builds only the workbench.

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
policy; nothing client-side is needed.

## What a tab does not do

- No [generation](../consumers/gen.md) or verification runs: both shell out to the
  deliverable's build and test commands, and `crate::sys::run_shell` has no shell.
- No [LSP](./lsp.md), no [ACP](./acp.md), no external [executors](./acp.md#agents):
  all three need a process or a socket.
- No rendering to `.svg` or `.png`. See [diagrams](#diagrams).
- No file watching, and no writing through to a machine.
- No [benchmark](../benchmark/benchmark.md).
