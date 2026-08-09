# CLI

The CLI is the command line frontend over the [compiler](../compiler/compiler.md). It runs the
build, inspects the [graph store](../compiler/graph.md), and hosts the other frontends.

## Commands

### jazyk init

`jazyk init [--mcp AGENT]` initializes the current directory as a project root:

- Writes a minimal `jazyk.toml` naming a starter layout: the docs glob
  (`docs/**/*.md`), the root document (`docs/README.md`), and the
  [deliverable directory](../compiler/project-settings.md#generation)
  (`deliverable`). When one already exists there, it warns and leaves the file
  unchanged. When discovery previously resolved to a parent project, the note says
  so: the nearest `jazyk.toml` wins, so the new file takes over from here down.
- Scaffolds the layout the file names: creates `docs/` and `deliverable/`, and
  seeds `docs/README.md` with a TODO placeholder so the first compile has input.
  Existing directories and files stay untouched. When `jazyk.toml` already
  existed, the scaffold step is skipped: the layout is that project's own
  business.
- Offers [MCP](./mcp.md) integration for a coding agent: a numbered choice of
  `none`, Claude Code (`.mcp.json`), Cursor (`.cursor/mcp.json`), VS Code
  (`.vscode/mcp.json`), or Gemini CLI (`.gemini/settings.json`). The chosen file
  gains a `jazyk` server entry running `jazyk mcp graph`; an existing file is merged,
  never overwritten, and an existing `jazyk` entry warns and stays. `--mcp AGENT`
  (`claude`, `cursor`, `vscode`, `gemini`, `none`) skips the prompt, and a
  non-interactive stdin skips it too, so scripts never hang.

The server entry is read-only; add `--write` to its `args` to hand the agent the
[write tools](../compiler/tools.md#write-tools). Exit `0` when something was set up,
`1` when nothing was written.

### jazyk compile

`jazyk compile [path...]` runs the [reconciler](../compiler/reconciler.md) to a
[fixed point](../compiler/reconciler.md#convergence).

Live trace:

- Default: one line per [turn](../compiler/turns.md) round, showing the tool calls with
  condensed arguments, the condensed results, and the model's reasoning text. See
  [trace events](../compiler/turns.md#trace-events). Endpoint trouble prints here too:
  retries, rate-limit waits, and the sticky fallbacks (codec downgrade, streaming,
  dropped `temperature`) are trace events, not stray stderr, so every frontend sees
  them.
- `--verbose`: additionally prints the full [context packs](../compiler/context.md) and raw
  payloads, plus a line per model call for the request (messages, size) and its answer
  (elapsed, completion tokens), and the section each accepted call names.
- `--quiet`: prints only the final summary.

Every build leaves a transcript: `compile`, `check`, `gen`, and `test` persist their
trace as one JSON-lines file under `<out>/trace/`, the same format the
[GUI](./gui.md#jobs) writes and lists, so a CLI build shows up in the GUI's activity
panel. The metadata line carries `source: cli` and no job id. `--quiet` still
persists the transcript; only the terminal rendering is quiet.

Exit code: `0` on convergence, non-zero when work was parked.

### jazyk check

Compile, then exit non-zero if open [diagnostics](../compiler/model.md#node-types) of severity
`error` exist. The CI gate.

### jazyk watch

Recompile on file change, using native file events (with a polling fallback when no
watcher is available). Event bursts debounce, and a fingerprint over the matched
documents decides whether a build actually runs, so editor temp files and the out
directory's own writes never trigger one. The same loop as `compile`: each change feeds
the [dirty set](../compiler/reconciler.md#dirty-set), so unchanged documents are not
revisited. See [incremental builds](../compiler/reconciler.md#incremental-builds).

A build that ends `incomplete` (work parked, e.g. by a transient endpoint outage)
retries on its own with backoff (30s doubling to 5 minutes, reset by any file change)
instead of idling until the next edit. Unfinished work is never silent, and watch is
the loop that owns resuming it.

### jazyk monitor

`jazyk monitor [--json] [--once]` watches the same surfaces `watch` does but performs
nothing: on every state change it prints the ready work from
[the task queue](../compiler/reconciler.md#the-task-queue) and which MCP tool begins
it, then goes quiet until the next change. One block per notice; `--json` prints one
JSON object per line instead. Output is flushed per notice, so a pipe reads events as
they happen. E.g.:

```
jazyk: 1 compilation task ready
  reconcile docs/template.md (2 dirty sections, 1 stale anchor)
  → call compilation_tasks on the jazyk MCP server to begin
```

This is the external agent's trigger, in two shapes:

- Stream: the agent runs `jazyk monitor` under its own process monitor (any harness
  facility that turns a background command's output lines into events) and acts on
  each notice through [MCP](./mcp.md#the-work-loop). The process runs until killed.
- One shot: `--once` blocks silently until the queue holds ready work, prints that
  one notice, and exits 0. Built for "wake me when there is something to do": a
  background shell awaiting the exit, or a script chaining
  `jazyk monitor --once && <act>`.

`watch` is the same trigger wired to the internal loop instead; the notice an agent
reads is the work item the internal loop would run.

In `manual` [mode](../compiler/reconciler.md#modes-and-releases), gated work prints
as "awaiting release" instead of prompting the agent to begin, and the notice fires
again when the release lands. `--once` waits for work that is actually actionable,
so a released click is what makes it exit.

### jazyk release

`jazyk release [compile|generate]` records a
[release](../compiler/reconciler.md#modes-and-releases): approve the pending changes
for the named stage (both when unnamed) without running anything. The watchers wake,
whichever worker is attached does the work. This is the scriptable form of the GUI's
release button. An explicit `jazyk compile` or `jazyk gen` is itself a release for
its stage: a typed command is an approval. The generate stage covers
[binding](../consumers/bind.md#when-binding-runs) too; decompilation releases through
[`jazyk decompile`](#jazyk-decompile), never through `release`.

### jazyk status

Summarize `status.yaml` (see [storage layout](../compiler/graph.md#storage-layout)):

- generation counter,
- [coverage](../compiler/reconciler.md#coverage) percentage,
- open diagnostics by severity,
- parked work,
- the [unclaimed report](../consumers/bind.md#the-unclaimed-report): deliverable
  files no binding names.

### jazyk context

`jazyk context <target> [--focus parents=2,mentions=1,requirements=2] [--budget N]` prints the
rendered [context pack](../compiler/context.md) for a target, with its
[expansion handles](../compiler/context.md#expansion-handles). This is the debug window into the
context engine: what this command prints is exactly what a turn sees.

`<target>` is a section reference, an entity id, or a requirement id. See
[request](../compiler/context.md#request). E.g.:

```
jazyk context ent:shopping-cart --focus mentions=1,requirements=2 --budget 8000
```

### jazyk query

`jazyk query <text>` runs the [search tool](../compiler/tools.md#read-tools) and prints the
matches, one `{id, name, definition}` line each.

### jazyk gen

`jazyk gen [entity...]` runs the built-in [generation](../consumers/gen.md) worker.
It performs owed [bind tasks](../consumers/bind.md) first (search the deliverable,
find or write the test per requirement, record the row), then generates: each entity's
part of the deliverable in one bounded task, making its `unimplemented` bound tests
pass, written into the configured
[deliverable directory](../compiler/project-settings.md#generation) and recorded as a
manifest in the [ledger](../consumers/gen.md#the-ledger). With no arguments it covers
every entity that has at least one requirement, leaf entities first, skipping entities
whose facts are unchanged (`--force` regenerates everything). Dense entities generate
in parts. The layout and run commands are the generator's choices, derived from the
documents; the [medium](../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)
is decided once per deliverable, before the first task. When that medium must be built,
the run ends by running [the build](../consumers/gen.md#the-build) and says what it
produced, or what it said when it failed. `jazyk codegen` and `jazyk testgen` are
deprecated aliases.

### jazyk test

`jazyk test [target...]` runs [verification](../consumers/gen.md#runners). With no
arguments it processes every runnable ledger row; entity ids select their requirements'
rows; requirement ids select rows directly. `programmatic` rows execute their recorded
command (exit 0 is a pass); `llm` rows run the in-process harness against the criteria.
`--kind programmatic|llm` filters; `--force` also reruns `verified` rows; `--list`
prints the derived status table without running; `--audit` rebuilds the ledger from the
artifact markers. Exit 0 when every targeted row is `verified`, 1 otherwise.

### jazyk decompile

`jazyk decompile [path...]` runs [decompilation](../consumers/decompile.md): draft
documents describing what the code under the named scopes does (default: the whole
[unclaimed report](../consumers/bind.md#the-unclaimed-report)). The command records
the decompile release for its scopes; with an agent attached and preferred by the
[dispatch](../compiler/reconciler.md#dispatch) preference, the agent's watcher does
the drafting, otherwise the built-in worker runs each scope as a turn with read-only
file tools over the deliverable. Drafts land in the docs tree carrying `unratified`
diagnostics until edited ([ratification](../consumers/decompile.md#ratification)).
Decompilation has no auto mode; this command and the GUI's decompile action are the
only triggers.

### jazyk docsgen

`jazyk docsgen` renders the per-entity requirements documents into `<out>/docsgen/` on
demand, without compiling. The same render runs after every committed changeset. See
[documentation generation](../consumers/docsgen.md#the-requirements-document).

### jazyk viewer

`jazyk viewer [--out FILE]` renders the graph into one self-contained HTML file
(default `<out>/graph.html`). See [viewer](./viewer.md).

### jazyk gui

`jazyk gui [--port N] [--no-open] [--watch] [--gui-dist DIR] [--no-token]` starts the
[GUI](./gui.md): one local server with the web app, the JSON API, the event stream, and
the language server over WebSocket, then opens the browser. `--no-open` skips opening
the browser. `--watch` starts in the automatic
[compile mode](./gui.md#workflow-modes) (default:
changes queue and compiling is an explicit click). Binds `127.0.0.1` only.

### jazyk mcp graph

Start the [MCP server](./mcp.md) on stdio. Read tools by default; `--write` adds the
[write tools](../compiler/tools.md#write-tools).

### jazyk lsp

`jazyk lsp` starts the [language server](./lsp.md) on stdio. Read-only: it serves the last
committed graph, and a `compile` or `watch` rebuild refreshes it.

### jazyk benchmark

Grade whether the configured model is good enough to compile Jazyk. See
[benchmark](../benchmark/benchmark.md). Results land in `<out>/benchmark/results.yaml`.

## Common options

- `--llm-base-url URL`: the LLM endpoint.
- `--model M`: the model to use.
- `--api-key K`: sent as a bearer token.
- `--out DIR`: the out directory (default `jazyk-out/`).

## Help

- `jazyk --help` (or `-h`, or bare `jazyk help`) prints the top-level usage: one line per
  command plus the common options.
- `jazyk <command> --help` prints that command's usage: its arguments, the options it
  honors (only those), and its exit codes where they carry meaning. `jazyk help <command>`
  is equivalent.
- Help prints to stdout and exits `0`. A missing or unknown command prints the top-level
  usage to stderr and exits `2`.

## Project discovery

The CLI walks up from the working directory to the nearest `jazyk.toml` and treats that
directory as the project root. A [redirect](../compiler/project-settings.md#redirect)
found above the working directory is a boundary, not a capture: discovery stops there
and the command runs ad hoc at the working directory. Explicit `[path...]` arguments
skip discovery and run ad hoc on those files. The out directory defaults to
`<root>/jazyk-out/`.
