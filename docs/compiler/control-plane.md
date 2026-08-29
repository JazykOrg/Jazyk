# Control plane

The [goal board](./reconciler.md#goal-derivation) says what work exists. The control
plane says whether anyone may act on it and who is acting. It is one file plus two
directories in the out directory, so every consumer (the internal loop, the GUI, an
agent over MCP, `jazyk monitor`) reads the same intent the same way the board is the
same everywhere. Without it, each frontend invents its own policy in process memory and
workers fight.

## Modes and releases

`control.yaml` holds the workflow policy and the standing approvals:

- `compile` and `generate`: `auto` or `manual` (the default). Defaults come from the
  project's [`[workflow]`](./project-settings.md#workflow) section; the file records
  runtime changes (a GUI toggle, a CLI flag) and survives restarts.
- `released.compile`: a map of document to the content hash approved for
  reconciliation. `released.generate`: the graph generation number approved for
  generation and [binding](../consumers/bind.md). `released.decompile`: the list of
  scopes approved for [decompilation](../consumers/decompile.md#triggering).

In `auto` mode nothing is gated. In `manual` mode a change still updates the board
(dirty sets are hashing, no model runs), but its goals carry `blocked {on: release}`
until a release approves them:

- A `place-anchors` or `reconcile-section` goal is blocked until `released.compile`
  maps its document to the document's current content hash. Editing after a release
  re-gates exactly that document.
- A `generate` goal is blocked until `released.generate` equals the graph's current
  generation. A commit moves the generation, so new graph facts gate new generation
  work until the next release.
- A `bind` goal gates with generation: it writes test files into the deliverable, so
  the same `released.generate` opens it
  ([binding](../consumers/bind.md#when-binding-runs)).
- Decompile drafts stay outside the board and are gated until `released.decompile`
  names their scope. `jazyk decompile` and the GUI's decompile action record it; a
  submitted draft covering a scope consumes it
  ([decompilation](../consumers/decompile.md#triggering)).
- `rejudge-pair`, `review-entity`, `retrace`, `conform-instance`, and `verify` goals
  are never gated: judgment is the second half of a reconciliation already approved,
  and verification only exists for recorded generation work. GC goals are never gated
  either: they follow from approved content, and the prose they propose goes through
  ratification. The fix-fail-reverify loop stays self-driving.
- `ratify` and `answer` goals wait on a human in every mode; a release does not touch
  them.

A release records the approval: `jazyk release [compile|generate]` from the CLI, the
compile and generate actions in the [GUI](../frontends/gui.md#workflow-modes), or an
explicit `jazyk compile` or `jazyk gen` (a typed command is an approval; `manual`
means nothing acts unprompted, not that prompts need a second prompt). Every watcher
wakes on it: the control file is a watched surface of
[`await_changes`](../frontends/mcp.md#the-work-loop) and `jazyk monitor`, so the
release a user clicks is the trigger an attached agent acts on. In `manual` mode the
GUI shows the next batch's prompt before the release
([preview](./sessions.md#preview)), so the approval is of what the model will see.

Gated work is visible everywhere but actionable nowhere: the board renders the block
with the release that would lift it, `begin_goals` refuses a blocked goal with an
`awaiting-release` error naming that release, and the notices `monitor` prints say
"awaiting release" instead of prompting the agent to begin. Blocked goals count in the
[verdict](./compilation.md#convergence) as `blocked` and never hold a
[cone](./reconciler.md#cones).

## Workers and leases

Two directories make the actors and their claims visible:

- `workers/<id>.yaml`: one file per attached worker session. `kind` (`internal`,
  `gui`, `agent`), `client` (the MCP client name when there is one), `pid`,
  `startedAt`, `heartbeatAt`, and the batch currently held. A worker refreshes its
  heartbeat while alive; a file whose heartbeat is older than 90 seconds is stale
  and swept on the next board derivation. Registration happens at MCP `initialize`
  for the lifecycle servings, at job start in the GUI, and for the lifetime of a
  `jazyk compile`/`gen`/`test` run.
- `leases/<batch>.yaml`: an exclusive claim on one goal batch, naming the goals it
  holds. `begin_goals` takes the lease for the batch it claims (create-new semantics,
  so exactly one claimant wins); `done` and `abandon_goals` release it. A lease names
  its worker and expires (default 120 seconds, refreshed by any tool call on the open
  batch), so a dead agent's claim evaporates instead of wedging the board. The board
  shows `claimedBy` on leased goals and consumers skip them.

The internal loop holds one coarse `build` lease for a whole run instead of per-batch
leases. The two granularities exclude each other: `begin_goals` refuses while a live
build lease exists (`build-running`), and a build refuses to start while any live batch
lease exists, naming the holder. The store's commit lock stays underneath as the
correctness backstop; leases exist so work is not duplicated, not to make commits safe
(they already are).

## Sequential builds

Compilation is sequential. One build runs at a time: the build lease enforces it. Within
a build one session runs at a time, each with one batch, and the board re-derives after
every commit before the next batch is chosen ([scheduling](./reconciler.md#scheduling)).
An external consumer over MCP is the same one-at-a-time actor: `begin_goals` claims one
batch, `done` finishes it and may claim the next
([compilation over MCP](../frontends/mcp.md#compilation-over-mcp)). Nothing in the
design depends on parallel sessions.

## Executors

Who runs a session is the executor: an [ACP agent profile](../frontends/acp.md#agents).
One global profile serves every kind, with overrides per goal kind or per goal class in
the project's [`[executors]`](./project-settings.md#executors) table, so extraction can
run cheap while GC judgment runs on the strongest model available. E.g.:

```toml
[acp]
agent = "embedded"

[executors]
gc = "claude-code"
reconcile-section = "embedded"
```

Resolution for one goal kind, first match wins: the `--agent` flag, `JAZYK_ACP_AGENT`,
`[executors].<kind>`, `[executors].<class>` for the kind's class, `[acp] agent`, the
built-in default. The scheduler resolves the executor per kind before it batches, and
goals whose kinds resolve to different executors never share a batch
([batching](./reconciler.md#batching)), so every batch has exactly one executor. The
resolved profile is recorded on the session's trace and its worker file, and per-kind
cost accounting in `status.yaml` (`costs.by_kind`, `costs.by_class`) is what makes the
choice informed.

## Dispatch

`worker` in `control.yaml` (`internal`, `agent`, or `any`, default `any`) resolves
who acts on a release from the GUI:

- `agent`: the GUI records the release and stops; the attached agent's watcher does
  the work. No agent registered means the GUI says so and offers the internal run.
- `internal`: the GUI runs its own job.
- `any`: prefer a live registered agent, fall back to internal. Leases make the race
  harmless either way.
