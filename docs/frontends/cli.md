# CLI

The CLI is the command line frontend over the [compiler](../compiler/compiler.md). It runs the
build, inspects the [graph store](../compiler/graph.md) and the
[goal board](../compiler/reconciler.md#goal-derivation), and hosts the other frontends.

## Commands

### jazyk init

`jazyk init [--mcp AGENT]` initializes the current directory as a project root:

- Writes a minimal `jazyk.toml` naming a starter layout: the docs glob
  (`docs/**/*.md`), the root document (`docs/README.md`), and the
  [deliverable directory](../compiler/project-settings.md#generation)
  (`deliverable`). When one already exists there, it warns and leaves the file
  unchanged. When discovery resolved to a parent project before the file was written,
  the note says so: the nearest `jazyk.toml` wins, so the new file takes over from here
  down.
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
- Asks which [agent](./acp.md#agents) does the AI work: the `embedded` one, or an
  external agent whose command line jazyk knows (`codex`, `claude`, `opencode`). The
  answer lands in the project's `[acp]` section, with the command line recorded for
  an external agent. `--agent NAME` (or `none`) skips the prompt, and a
  non-interactive stdin keeps the default.
- For the embedded agent, asks which model. Jazyk asks the configured endpoint what
  it serves and lists that, so the choice is between models that exist rather than
  remembered names; a blank answer keeps the resolved one, and a typed name is taken
  as given. The answer lands in `[llm] model`. An unanswering endpoint is reported,
  not fatal. Inside an IDE the same choice is available per session through
  [session config options](./acp.md#choosing-a-model).
- Offers [ACP registration](./acp.md#registration) the same way: a numbered choice of
  `none` or one of the supported clients, running
  [`jazyk acp install`](#jazyk-acp) for the chosen one. `--acp IDE` (any client name,
  or `none`) skips the prompt, and a non-interactive stdin skips it too.

The server entry is read-only; add `--write` to its `args` to hand the agent the
[write tools](../compiler/tools.md#write-tools). Exit `0` when something was set up,
`1` when nothing was written.

### jazyk compile

`jazyk compile [path...]` runs one [build](../compiler/compilation.md#a-build). The
[reconciler](../compiler/reconciler.md) derives the goal board from the documents, the
graph, the ledger, and the [change records](../compiler/graph.md#change-records); the
scheduler [batches](../compiler/reconciler.md#batching) ready goals; and
[sessions](../compiler/sessions.md) resolve them until the board
[converges](../compiler/compilation.md#convergence). Sessions run as
[ACP worker sessions](./acp.md#worker-sessions) against the configured
[agent](../compiler/project-settings.md#acp), one at a time (compilation is sequential;
the build lease enforces it); the agent process lives for the run. `--sessions N` runs
at most N sessions and stops with an honest `incomplete`, so a project advances one
batch at a time and any moment can be copied for a
[snippet](../benchmark/benchmark.md#snippets-from-a-real-project).
[Executor overrides](../compiler/project-settings.md#executors) per goal kind or class
send some sessions to a different agent.

Terminal output, in order:

- The board summary first, once the documents are parsed and the board is derived: the
  goal count, the count per kind, and the blocked count. E.g.:

```
compile: 27 goals (12 reconcile-section, 9 rejudge-pair, 6 retrace), 3 blocked
```

- The live trace while sessions run (below).
- One `gc burst:` line as each [GC burst](../compiler/compilation.md#compile-and-garbage-collection)
  starts, naming the goal kind, the target, and the count against the
  [limit](../compiler/graph.md#limits) that opened it. E.g.:

```
gc burst: abstract-entity ent:order (54 > 50)
```

- The verdict last, with its counts. `converged` when no mandatory goal of either
  class is open or failed and the [checks](../compiler/compilation.md#checks) are
  clean; the blocked and optional counts ride beside it. `incomplete` otherwise, with
  the counts of open, failed, blocked, and optional goals. E.g.:

```
converged, 2 blocked, 1 optional advised
incomplete: 3 open, 1 failed, 2 blocked, 5 optional advised
```

Live trace:

- Default: the session lifecycle lines (`batchStart`, `sessionStart`, `sessionDone`,
  `sessionFailed`), one line per session round showing the tool calls with condensed
  arguments, the condensed results, and the model's reasoning text, and one line per
  goal resolved (with its justification) or failed (with its reason). See
  [trace events](../compiler/sessions.md#trace-events). Endpoint trouble prints here
  too: retries, rate-limit waits, and the sticky fallbacks (codec downgrade, streaming,
  dropped `temperature`) are trace events, not stray stderr, so every frontend sees
  them.
- `--verbose`: additionally prints every session's assembled
  [prompt](../compiler/sessions.md#the-prompt) (the goals block and the
  [loaded set](../compiler/context.md#the-loaded-set) as the model receives them),
  every `goal` event as it fires (opened with its cause, resolved with its
  justification: the cascade as it happens), the raw payloads, a line per model call
  for the request (messages, size) and its answer (elapsed, completion tokens), and the
  section each accepted call names.
- `--quiet`: prints the board summary, the `gc burst:` lines, and the verdict only.

Every build leaves a transcript: `compile`, `check`, `gen`, and `test` persist their
trace as one JSON-lines file under `<out>/trace/`, the same format the
[GUI](./gui.md#jobs) writes and lists, so a CLI build shows up in the GUI's activity
panel. The metadata line carries `source: cli` and no job id. `--quiet` still
persists the transcript; only the terminal rendering is quiet. After the build,
[`jazyk ripple`](#jazyk-ripple) replays how every goal came to exist.

An immediately repeated `jazyk compile` prints `compile: 0 goals` and the verdict, and
makes no LLM call ([incremental builds](../compiler/compilation.md#incremental-builds)).

Exit codes: `0` on `converged` (the blocked and optional counts do not change it),
`1` on `incomplete` (an open or failed mandatory goal, parked work, or a failing
check), `2` on a usage error (see [help](#help)).

### jazyk check

Compile, then exit non-zero if open [diagnostics](../compiler/model.md#node-kinds) of
severity `error` exist, the findings of the [checks](../compiler/compilation.md#checks)
included. The CI gate. Exit `1` when the build ends `incomplete` or an open `error`
diagnostic remains after it, `0` otherwise.

### jazyk watch

Recompile on file change, using native file events (with a polling fallback when no
watcher is available). Event bursts debounce, and a fingerprint over the matched
documents decides whether a build actually runs, so editor temp files and the out
directory's own writes never trigger one. The same loop as `compile`: each change feeds
the [dirty set](../compiler/reconciler.md#dirty-set), so unchanged documents are not
revisited. See [incremental builds](../compiler/compilation.md#incremental-builds).

Terminal output per build is the board summary, then one line per goal, then the
verdict. A goal line says what opened (the kind, the target, the cause), what session
took it, and how it ended: resolved with its justification, failed with its reason, or
parked. `gc burst:` lines print as in `compile`. `--verbose` prints the full `compile`
trace instead. E.g.:

```
opened   g:reconcile-section:docs/orders.md#/orders/holds  (g87 via section-dirty)
taken    g:reconcile-section:docs/orders.md#/orders/holds  b87-1
resolved g:reconcile-section:docs/orders.md#/orders/holds  req:orders-6 revised (quote, statement, transition guard)
opened   g:rejudge-pair:req:orders-6~req:payment-9  (g88 via entities)
```

A build that ends `incomplete` (work parked, e.g. by a transient endpoint outage)
retries on its own with backoff (30s doubling to 5 minutes, reset by any file change)
instead of idling until the next edit.
[Parked goals](../compiler/reconciler.md#parked-and-failed) resume first. Unfinished
work is never silent, and watch is the loop that owns resuming it.

### jazyk monitor

`jazyk monitor [--json] [--once]` watches the same surfaces `watch` does but performs
nothing: on every state change it prints the ready goals on the
[board](../compiler/reconciler.md#readiness) and which MCP tool claims them, then goes
quiet until the next change. One block per notice; `--json` prints one JSON object per
line instead. Output is flushed per notice, so a pipe reads events as they happen.
Blocked goals print with their reason (an unanswered prompt, a ratification proposal, a
gated release), so the notice says what a human owes. E.g.:

```
jazyk: 3 goals ready, 1 blocked
  reconcile-section docs/template.md#/template/usage (section dirty, 1 stale anchor)
  reconcile-section docs/template.md#/template/install (section dirty)
  retrace view:usecase/holds (member req:orders-6 gone)
  blocked: answer diag:decision-2 (awaiting a human answer)
  → call goals on the jazyk MCP server, then begin_goals to claim a batch
```

This is the external agent's trigger, in two shapes:

- Stream: the agent runs `jazyk monitor` under its own process monitor (any harness
  facility that makes a background command's output lines into events) and acts on
  each notice through [MCP](./mcp.md#the-work-loop). The process runs until killed.
- One shot: `--once` blocks silently until the board holds a ready goal, prints that
  one notice, and exits 0. Built for "wake me when there is something to do": a
  background shell awaiting the exit, or a script chaining
  `jazyk monitor --once && <act>`.

`watch` is the same trigger wired to the internal loop instead; the notice an agent
reads is the goal batch the internal loop would run.

In `manual` [mode](../compiler/control-plane.md#modes-and-releases), gated goals print
as "awaiting release" instead of prompting the agent to begin, and the notice fires
again when the release lands. `--once` waits for a goal that is actually claimable,
so a released click is what makes it exit.

### jazyk release

`jazyk release [compile|generate]` records a
[release](../compiler/control-plane.md#modes-and-releases): approve the pending changes
for the named stage (both when unnamed) without running anything. The watchers wake,
whichever worker is attached does the work. This is the scriptable form of the GUI's
release button. An explicit `jazyk compile` or `jazyk gen` is itself a release for
its stage: a typed command is an approval. The generate stage covers
[binding](../consumers/bind.md#when-binding-runs) too; decompilation releases through
[`jazyk decompile`](#jazyk-decompile), never through `release`.

### jazyk status

Summarize `status.yaml` (see [storage layout](../compiler/graph.md#storage-layout)) and
the board:

- the store `version` and the generation counter,
- the last verdict with its counts,
- the board counts: open goals by class (compile, GC), blocked, parked,
  failed, optional advised. The board is derived from disk the same way `compile`
  derives it, so on a tree with pending edits `status` shows the goals the next build
  will run,
- [coverage](../compiler/compilation.md#coverage) percentage,
- open diagnostics by severity,
- the medium warning, when the ledger's medium and the recorded run commands disagree
  ([the medium](../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)),
- the shape line: the entity count per depth of the containment tree (the
  [scope root](../compiler/concepts/levels.md#the-scope-root) at depth 0, its
  parentless entities at depth 1, and so on), then the fan-out histogram: how many
  nodes with children (the scope root included) hold how many direct children, in
  bands against the `children-per-entity` limit
  ([limits](../compiler/graph.md#limits)): at or under soft, over soft and at or
  under hard, over hard. A node over hard is what the `level-shape` check
  ([checks](../compiler/compilation.md#checks)) reports and what a mandatory
  `abstract-entity` goal on the board line is open for,
- the last build's cost (`costs`: sessions, tokens, the share per goal kind),
- the [unattached remainder](../consumers/gen.md#the-unattached-remainder): generated
  mass no requirement claims, summed over the ledger, with the worst entity's ratio,
- the [unclaimed report](../consumers/bind.md#the-unclaimed-report): deliverable
  files no binding names.

E.g.:

```
store: version 2, generation 413
verdict: converged, 2 blocked, 1 optional advised
board: 0 open (0 compile, 0 gc), 2 blocked, 0 parked, 0 failed, 1 optional
coverage: 96% (185 of 193 sections)
diagnostics: 1 error, 4 warnings
shape: 3 / 11 / 42 nodes per depth; fan-out 2-9: 12, 10-15: 2, over 15: 0
cost: 41 sessions, 310k tokens (78% reconcile-section)
unattached: 3 file(s), 120 line(s) (worst cart at 0.32)
unclaimed: 3 file(s) no binding names (`jazyk decompile` drafts docs for them)
  - src/cart.rs
  - src/coupon.rs
  - src/tax.rs
```

### jazyk preview

`jazyk preview [goal|target|batch]` renders the next session's prompt exactly as the
model would receive it: the agent contract, the active skills, the project block, the
goals block for the batch, the loaded set with its handles, and the worker protocol line
(see [the prompt](../compiler/sessions.md#the-prompt)). With a goal id, it renders the
batch that goal would join; with a target (a node id, a section reference, a document
path), the batch of the first ready goal on that target; with a batch id
(`b<generation>-<n>`, as the [board](../compiler/reconciler.md#batching) lists them),
that batch's prompt; without an argument, the batch the scheduler would claim next.
A goal that is not ready yet renders all the same, with a `not ready:` line and the
readiness reason above the prompt, so its session can be
inspected before its tier arrives. The rendering is deterministic, so what `preview`
prints is what the session will spend; it makes no LLM call and writes nothing. The
[GUI](./gui.md) shows the same pane before a release in `manual` mode
([preview](../compiler/sessions.md#preview)).

`ratify` and `answer` goals have no session; for those `preview` prints what the human
owes instead of a prompt.

Exit codes: `0` when a prompt was rendered, `1` when nothing rendered (an empty board,
an unknown goal, a target with no goal, a blocked-on-human goal); the reason prints.

### jazyk explain

`jazyk explain [goal|target]` is a rendering over derivable state: the board, the
change records, and the graph. It makes no LLM call and writes nothing.

- For a goal: the change record that produced it (kind, subject, `via`, the detail),
  its cause (generation, mutation, the edge or computation), its class and whether it
  is mandatory, its [readiness](../compiler/reconciler.md#readiness) (the tier, ready
  or the reason it waits, rendered as a sentence), what blocks it, and its hints. E.g.:

```
g:retrace:view:usecase/holds  retrace, compile, mandatory, open
  change: view-member-gone req:orders-6 (deleted in g409, reason: duplicate)
  cause:  g409 mutation 2 via members
  ready:  tier 2; waits for g:reconcile-section:docs/orders.md#/orders/holds (tier 1, same cone)
  hints:  load view:usecase/holds; skill flow-views
```

- For a target (a node id, a section reference, a document path): the cone of goals a
  change to it would open, walking the stored references and the computed derivations
  from it, each with the kind, the target, and the edge it is reached through, plus
  the [derived data](../compiler/graph.md#derived-data) a commit would recompute. E.g.:

```
ent:order: a change here opens
  review-entity ent:order              via entity-changed
  conform-instance ent:order-4711      via instantiation
  retrace view:usecase/holds           via members
  retrace view:sequence/holds          via members
  recomputed at commit: rel:order~order-item, sm:order, view:class/commerce
```

- Without an argument: the whole board, one line per goal (id, class, mandatory or
  optional, state, readiness), ready goals first, then waiting, blocked, parked,
  failed.

Exit codes: `0`; `1` when the goal or target is unknown.

### jazyk ripple

`jazyk ripple [target|generation|doc] [--back]` prints the ripple DAG rooted at a
change: every generation the root led to, the goals each generation opened and the
sessions that resolved them, each with its cause and its one-line justification. It is
a rendering over the [journal](../compiler/graph.md#journal); nothing is stored for it.
Without a root it walks the last build: the cascade rooted at the generation the build
opened with, the whole-build report.

- For a target (a node id): the last cascade that touched it, from the human edit at
  its root through the generations that followed.
- For a generation (`g87`, or `87`): the full tree forward from it. Rooted at the
  generation a build opened with, this is the whole-build report: the causality DAG,
  the cost totals (sessions, tokens, per goal kind), and the parked and failed goals
  with their reasons. A generation doubles as the whole-build report this way.
- For a document path: the cascade rooted at the last `edit` entry that dirtied it.
- `--back`: causes instead of consequences. From the root, walk backward through the
  `updated` markers and the recorded causes to the human edit that started everything.

The trace a one-sentence edit leaves (`orders.md`: "held orders expire after 21 days"
becomes "30 days"):

```
edit g87 docs/orders.md#/orders/holds (human)
└─ reconcile-section docs/orders.md#/orders/holds g88: req:orders-6 revised (quote, statement, transition guard)
   │  recomputed at commit: sm:order (held→expired guard), view:sequence/holds
   ├─ rejudge-pair req:orders-6~req:payment-9 g89: consistent
   └─ bind req:orders-6 g90: row stale (requirement-changed), test rewritten, no file implements 30 days → unimplemented
      └─ generate ent:order g91: files rewritten
         └─ verify req:orders-6 g92: pass
gc: no goals derived
converged: 4 sessions, 2 recomputes, 29k tokens
```

Every line is a journal entry; every indent is a goal with its cause and justification
on record. The `recomputed at commit` line is
[derived data](../compiler/graph.md#derived-data) following the requirement without a
session. Parked and failed goals print after the tree with their reasons; a build with
none says so.

Exit codes: `0`; `1` when the root is unknown (no journal entry touched the target, no
such generation, no `edit` entry for the document) or the journal is empty.

### jazyk context

`jazyk context <target> [--depth N] [--expand HANDLE...]` prints what the
[`load`](../compiler/context.md#tools) tool renders for a target: the target in full,
its edges, each neighbor as a stub, and the [status block](../compiler/context.md#rendering)
of the resulting [loaded set](../compiler/context.md#the-loaded-set) with its
[handles](../compiler/context.md#policy). This is the debug window into the loaded set:
what this command prints is exactly what a session sees after `load({target, depth})`,
under the same context budget (a registry constant, see
[the limits registry](../compiler/graph.md#limits)). `--expand` follows the named handles before
printing, as `expand` would.

`<target>` is any node id (entity, requirement, view) or a section reference. E.g.:

```
jazyk context ent:shopping-cart --depth 2
jazyk context view:class/commerce --expand h:view:class/commerce:members
```

### jazyk query

`jazyk query <text>` runs the [search tool](../compiler/tools.md#read-tools) and prints the
matches, one `{id, name, definition}` line each.

### jazyk gen

`jazyk gen [entity...]` runs the built-in [generation](../consumers/gen.md) worker over
the ledger goals: [`bind`](../compiler/goals/bind.md), [`generate`](../compiler/goals/generate.md),
and [`verify`](../compiler/goals/verify.md) sit on the same board at tier 3. `bind` and
`generate` are gated by the generate
[release](../compiler/control-plane.md#modes-and-releases), and `jazyk gen` is that
release. `verify` is not release-gated (verification writes nothing into the
deliverable). It resolves owed `bind` goals first (search the deliverable, find or write
the test per requirement from its `statement`, record the row), then `generate` goals:
each entity's part of the deliverable in one bounded session, making its
`unimplemented` bound tests pass, written into the configured
[deliverable directory](../compiler/project-settings.md#generation) and recorded as a
manifest in the [ledger](../consumers/gen.md#the-ledger). With no arguments it covers
every entity that has at least one requirement, leaf entities first, grouped by
component where containment gives one, skipping entities whose facts are unchanged
(`--force` regenerates everything). Dense entities generate in parts. The layout and
run commands are the generator's choices, derived from the documents; the
[medium](../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)
is decided once per deliverable, before the first session. When that medium must be
built, the run ends by running [the build](../consumers/gen.md#the-build) and says what
it produced, or what it said when it failed. A choice the documents do not settle is
recorded as an `invented-choice` diagnostic graded by the scope of the invention.
`jazyk codegen` and `jazyk testgen` print a pointer to `jazyk gen` and exit `2`
without running anything.

### jazyk test

`jazyk test [target...]` runs [verification](../consumers/gen.md#runners): the
[`verify`](../compiler/goals/verify.md) goals. With no arguments it processes every
runnable ledger row; entity ids select their requirements' rows; requirement ids select
rows directly. `programmatic` rows execute their recorded command (exit 0 is a pass);
`llm` rows run the in-process harness, judging the criteria against the requirement's
`statement`. `--kind programmatic|llm` filters; `--force` also reruns `verified` rows;
`--list` prints the derived status table (one row per requirement: id, `statement`,
status) without running; `--audit` rebuilds the ledger from the artifact markers. Exit
0 when every targeted row is `verified`, 1 otherwise.

### jazyk decompile

`jazyk decompile [path...]` runs [decompilation](../consumers/decompile.md): draft
documents describing what the code under the named scopes does (default: the whole
[unclaimed report](../consumers/bind.md#the-unclaimed-report)). The command records
the decompile release for its scopes; with an agent attached and preferred by the
[dispatch](../compiler/control-plane.md#dispatch) preference, the agent's watcher does
the drafting, otherwise the built-in worker runs each scope as one draft session with
read-only file tools over the deliverable. Drafts land in the docs tree carrying
`unratified` diagnostics until edited ([ratification](../consumers/decompile.md#ratification)).
Decompilation is not a board goal and has no auto mode; this command and the GUI's
decompile action are the only triggers.

### jazyk docsgen

`jazyk docsgen` renders the documentation pages into `<out>/docsgen/` on demand, without
compiling: the per-entity requirements documents (each entity's definition, its
requirements with their quotes, its relationships, and the rendered images of its
relevant views), the [entity cards](../consumers/docsgen.md#entity-cards) under
`entities/`, the [level pages](../consumers/docsgen.md#level-pages) under `levels/`,
and the [diagram pages](../consumers/docsgen.md#diagram-pages) under
`diagrams/<kind>/`, every image linked relatively into `<out>/diagrams/`
([output layout](../compiler/diagrams.md#output-layout)). The summary line counts every
page written, all four kinds together. The [diagrams](../compiler/diagrams.md#rendering)
render with it, skipped for unchanged `.puml` content, so every image link resolves.
The same render runs after every committed changeset. See
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
[write tools](../compiler/tools.md#write-tools). `jazyk mcp <toolsets>` serves the
other [toolsets](./mcp.md#toolsets): `compile` claims goal batches
([compilation over MCP](./mcp.md#compilation-over-mcp)), `generate`, `verify`,
`decompile`, `benchmark`, `chat`.

### jazyk acp

`jazyk acp` starts the [IDE-facing ACP proxy](./acp.md#the-ide-proxy) on stdio. An
IDE spawns it as its "Jazyk" agent; it drives the configured downstream agent and
adds jazyk in between: tool injection, doc edit delegation, slash commands, build
status. Outside a jazyk project it is a transparent passthrough. Not meant to be run
by hand.

`jazyk acp install --ide <client>` registers Jazyk with an ACP client:
`zed`, `jetbrains`, `vscode`, `neovim`, `emacs`, `obsidian`, `acpx`, `marimo` (see
[registration](./acp.md#registration)). The editor may also be given positionally
(`jazyk acp install zed`). For a client whose config jazyk writes, the entry is
merged in place, leaving comments and other agents alone; for the rest, the snippet
to paste is printed. Exit `0` when the entry was written, was already current, or the
snippet was printed, `1` when a file could not be written.

### jazyk agent

`jazyk agent` starts the [embedded ACP agent](./acp.md#the-embedded-agent) on stdio:
a generic agent over the configured [LLM endpoint](../compiler/project-settings.md#llm)
with no jazyk knowledge. Jazyk spawns it when the `embedded` profile is selected. Not
meant to be run by hand.

### jazyk lsp

`jazyk lsp` starts the [language server](./lsp.md) on stdio. Read-only: it serves the last
committed graph and its rendered diagrams, and a `compile` or `watch` rebuild refreshes
it.

### jazyk benchmark

Grade whether the configured [agent](../compiler/project-settings.md#acp) and model
can drive jazyk's [sessions](../compiler/sessions.md). See
[benchmark](../benchmark/benchmark.md); cases per goal kind are deferred there. Results
land in `<out>/benchmark/results.yaml`, per-case transcripts under
`<out>/benchmark/trace/`. `jazyk benchmark [case...]` grades
[a subset](../benchmark/benchmark.md#running-a-subset) for iterating on one failure.
`jazyk benchmark --project <dir> --goal <goal-id>` runs one goal's session from a
copy of a real project, prints the outcome and the transcript path, and touches
nothing ([snippets](../benchmark/benchmark.md#snippets-from-a-real-project));
`--force` runs the goal even when the board holds it blocked.

## Common options

- `--agent NAME`: the [ACP agent profile](../compiler/project-settings.md#acp) to use.
- `--llm-base-url URL`: the LLM endpoint (used by the embedded agent).
- `--model M`: the model to use (used by the embedded agent).
- `--api-key K`: sent as a bearer token (used by the embedded agent).
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
