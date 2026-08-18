# Control plane

The [task queue](./reconciler.md#the-task-queue) says what work exists. The control
plane says whether anyone may act on it
and who is acting. It is one file plus two directories in the out directory, so every
consumer (the internal loop, the GUI, an agent over MCP, `jazyk monitor`) reads the
same intent the same way the queue is the same everywhere. Without it, each frontend
invents its own policy in process memory and workers fight.

## Modes and releases

`control.yaml` holds the workflow policy and the standing approvals:

- `compile` and `generate`: `auto` or `manual` (the default). Defaults come from the
  project's [`[workflow]`](./project-settings.md#workflow) section; the file records
  runtime changes (a GUI toggle, a CLI flag) and survives restarts.
- `released.compile`: a map of document to the content hash approved for
  reconciliation. `released.generate`: the graph generation number approved for
  generation and [binding](../consumers/bind.md). `released.decompile`: the list of
  scopes approved for [decompilation](../consumers/decompile.md#triggering).

In `auto` mode nothing is gated; the behavior is today's. In `manual` mode a change
still updates the queue (dirty sets are hashing, no model runs), but its tasks carry
`gated: true` until a release approves them:

- A `reconcile-document` task is gated until `released.compile` maps its document to
  the document's current content hash. Editing after a release re-gates exactly that
  document.
- A `generate-entity` task is gated until `released.generate` equals the graph's
  current generation. A commit moves the generation, so new graph facts gate new
  generation work until the next release.
- A `bind-requirement` task gates with generation: it writes test files into the
  deliverable, so the same `released.generate` opens it
  ([binding](../consumers/bind.md#when-binding-runs)).
- A `draft-document` task is gated until `released.decompile` names its scope.
  `jazyk decompile` and the GUI's decompile action record it; a submitted draft
  covering a scope consumes it ([decompilation](../consumers/decompile.md#triggering)).
- Review tasks and `verify-requirement` tasks are never gated: reviews are the
  second half of a reconciliation already approved, and verification only exists for
  recorded generation work. The fix-fail-reverify loop stays self-driving.

A release records the approval: `jazyk release [compile|generate]` from the CLI, the
compile and generate actions in the [GUI](../frontends/gui.md#workflow-modes), or an
explicit `jazyk compile` or `jazyk gen` (a typed command is an approval; `manual`
means nothing acts unprompted, not that prompts need a second prompt). Every watcher
wakes on it: the control file is a watched surface of
[`await_changes`](../frontends/mcp.md#the-work-loop) and `jazyk monitor`, so the
release a user clicks is the trigger an attached agent acts on.

Gated work is visible everywhere but actionable nowhere: task lists carry the flag,
`begin_compilation` and `begin_generation` refuse a gated target with an
`awaiting-release` error naming the release that would open it, and the notices
`monitor` prints say "awaiting release" instead of prompting the agent to begin.

## Workers and leases

Two directories make the actors and their claims visible:

- `workers/<id>.yaml`: one file per attached worker session. `kind` (`internal`,
  `gui`, `agent`), `client` (the MCP client name when there is one), `pid`,
  `startedAt`, `heartbeatAt`, and the task currently held. A worker refreshes its
  heartbeat while alive; a file whose heartbeat is older than 90 seconds is stale
  and swept on the next queue computation. Registration happens at MCP `initialize`
  for the task-lifecycle servings, at job start in the GUI, and for the lifetime of
  a `jazyk compile`/`gen`/`test` run.
- `leases/<task>.yaml`: an exclusive claim on one task. `begin_compilation` and
  `begin_generation` take the lease (create-new semantics, so exactly one claimant
  wins); `finish_*` and `abandon_*` release it. A lease names its worker and expires
  (default 120 seconds, refreshed by any tool call on the open task), so a dead
  agent's claim evaporates instead of wedging the queue. Task lists show `claimedBy`
  on leased tasks and consumers skip them.

The internal loop holds one coarse `build` lease for a whole run instead of per-task
leases: it processes levels in parallel and is not a peer picking tasks one at a
time. The two granularities exclude each other: `begin_*` refuses while a live build
lease exists, and a build refuses to start while any live task lease exists, naming
the holder. The store's commit lock stays underneath as the correctness backstop;
leases exist so work is not duplicated, not to make commits safe (they already are).

## Dispatch

`worker` in `control.yaml` (`internal`, `agent`, or `any`, default `any`) resolves
who acts on a release from the GUI:

- `agent`: the GUI records the release and stops; the attached agent's watcher does
  the work. No agent registered means the GUI says so and offers the internal run.
- `internal`: the GUI runs its own job, as today.
- `any`: prefer a live registered agent, fall back to internal. Leases make the race
  harmless either way.
