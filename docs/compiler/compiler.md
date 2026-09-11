# Compiler

The compiler maintains a persistent [semantic graph](./model.md) that mirrors the
project's documentation. Compiling means reconciling: bring the graph in line with the
documents, surface ambiguity and contradictions as [diagnostics](./model/diagnostic.md),
draw every [view](./model/view.md) as a [diagram](./diagrams.md), and leave everything
queryable for downstream consumers.

The graph is the build artifact. It is edited in place, never regenerated. Entities,
requirements, views, and diagnostics keep their identity across builds, so everything
downstream (generated code, tests, tickets, triage, pictures) stays bound. See
[identity](./concepts/identity.md). Nothing enters the graph without
[provenance](./model.md#provenance).

Compilation is a goal board. The harness derives goals from the documents and the graph,
and one agent resolves them in sessions, one goal batch at a time. A build is converged
when no mandatory goal is open or failed and the checks are clean. A rebuild with no
changes derives zero goals and makes zero LLM calls.

## Division of labor

The design splits work strictly between deterministic code and the model.

The harness owns everything that must never be wrong:

- [parsing](./parsing.md) and [alignment](./alignment.md) (section diffing and anchor
  relocation),
- identity: ids minted once, natural keys, redirects on merge, and the
  [graph store](./graph.md) with its [validation gates](./graph.md#validation-gates),
- dirtiness: the [dirty set](./reconciler.md#dirty-set) and the
  [change records](./graph.md#change-records) every commit leaves behind,
- [goal derivation](./reconciler.md#goal-derivation),
  [readiness](./reconciler.md#readiness), and [batching](./reconciler.md#batching):
  what work exists, in what order, in which session,
- [derived data](./graph.md#derived-data): relationships, state machines, default views,
- the [loaded set](./context.md#the-loaded-set): what the model sees, under a budget,
- [budgets](./sessions.md#budgets) and causality (the [journal](./graph.md#journal),
  the cause on every goal),
- [rendering](./diagrams.md#rendering) and
  [garbage collection](./graph.md#garbage-collection).

The model owns everything that requires judgment:

- extraction: statements, entities, edges, transitions, and attributes from a section,
- same-vs-different: whether a concept already exists in the graph (search before
  create), and merging when it does,
- severity of a finding, and the wording of definitions and statements,
- abstraction: splitting an entity into a parent and children, splitting a view into
  sub-views,
- view curation: what a flow view or a structural view includes,
- justifications: why a goal is done or why it cannot be, and marking sections covered
  or non-normative.

## Components

Seven components carry a build. Five are the compiler proper. The control plane and the
ACP bridge are shared with every frontend.

### The store

The [graph store](./graph.md) is the persistent home of the graph: one YAML shard per
node kind under `graph/`, section trees and coverage under `docs/`, the journal, and
`status.yaml`. It commits [changesets](./graph.md#changesets) atomically behind the
validation gates, recomputes [derived data](./graph.md#derived-data) on every commit,
writes the [change records](./graph.md#change-records) that goal derivation reads,
journals every generation, and runs the deterministic sweep half of
[garbage collection](./graph.md#garbage-collection). The store carries a
[version](./graph.md#store-version) in `status.yaml`. An out directory of another
version is archived whole to the sibling directory `<out>.bak` and the build starts from
the empty graph; there is no migration code. See
[storage layout](./graph.md#storage-layout).

### The goal board

The board is the set of goals derived from disk: the documents, the graph, the ledger,
and the change records. It is never stored. Any process computes the same board, an
interrupted build resumes anywhere, and a no-op rebuild derives an empty board. A goal
carries its kind, its class (`compile` or `gc`), whether it is mandatory, its target, the
change that is its evidence and its identity, the cause that opened it, its state, and
hints. Compile goals bring the graph in line with the documents
([`reconcile-section`](./goals/reconcile-section.md),
[`rejudge-pair`](./goals/rejudge-pair.md), [`retrace`](./goals/retrace.md), and the
rest). GC goals restructure it ([`split-view`](./goals/split-view.md),
[`abstract-entity`](./goals/abstract-entity.md),
[`dedupe-candidates`](./goals/dedupe-candidates.md), and the rest). See
[goal derivation](./reconciler.md#goal-derivation) and
[compile and garbage collection](./compilation.md#compile-and-garbage-collection).

### The scheduler

The scheduler picks the next batch: the highest ready [tier](./reconciler.md#readiness),
open goals grouped by [locality](./reconciler.md#batching) (one document, one entity
neighborhood, one view), filled until the context budget says stop. A GC goal is ready
only when no compile goal is open or parked in its target's cone
([GC gating](./reconciler.md#gc-gating)), so restructuring sees settled content and the
two classes interleave in bursts. Sessions run one at a time
([sequential builds](./control-plane.md#sequential-builds)). Hard thresholds
[escalate](./reconciler.md#escalation) optional goals to mandatory. Exhausted budgets
[park](./reconciler.md#parked-and-failed) goals for the next build.
[Flip detection](./reconciler.md#flip-detection) parks oscillation for a human.

### The serving

The serving is the [tool registry](./tools.md) as one session sees it: served in process
to the embedded agent and over stdio to any other agent (`jazyk mcp`), injected into
every session, so the tools have one implementation whoever calls them. The serving
maintains the [loaded set](./context.md#the-loaded-set) and renders its status into
every round, stages mutations and validates them as staged
([staged mutations](./sessions.md#staged-mutations)), previews the goals a mutation will
open ([bubbling](./reconciler.md#bubbling)), holds the
[repeated-call guard](./sessions.md#repeated-calls), and runs the batch gates at `done`
([commit](./sessions.md#commit)). The [goal tools](./tools.md#goal-tools) are how a
session resolves or fails a goal; the serving checks each claim against the kind's gate.
[Read tools](./tools.md#read-tools) are the public query surface (`jazyk mcp graph`).
[Write tools](./tools.md#write-tools) mutate the graph and are used by sessions, or by an
external agent given `--write`.

### The renderer

The [renderer](./diagrams.md) draws every view on every commit: one
[emitter](./diagrams.md#the-emitters) per view kind, each a pure function of the store
snapshot and the view, writing `<out>/diagrams/<kind>/<slug>.puml` and `.svg`, with
`.png` on request. Diagrams are projections. There are no diagram elements in the graph,
and a rendering cannot drift from it because every build recomputes it. Rendering runs
in process (`plantuml-little`, `resvg`): no Java, no external tool. See
[the renderer](./diagrams.md#the-renderer).

### The control plane

The [control plane](./control-plane.md) decides whether anyone may act and who is
acting: everything in `auto` mode, only released work in `manual`
([modes and releases](./control-plane.md#modes-and-releases)). Modes, releases, workers,
and leases are files in the out directory, so every frontend reads the same policy. The
build lease makes compilation sequential: one build at a time, one session at a time
within it ([workers and leases](./control-plane.md#workers-and-leases)).

### The ACP bridge

Every session runs as one worker session over the
[ACP bridge](../frontends/acp.md#worker-sessions): jazyk is the ACP client of one
configured agent (an external coding agent, or the generic
[embedded agent](../frontends/acp.md#the-embedded-agent)). All AI work takes this one
path. The executor is chosen per goal kind or per goal class through
[`[executors]`](./project-settings.md#executors), so extraction can run on a cheap model
while GC judgment runs on the strongest one available. An external agent can also drive
compilation itself by claiming goal batches over
[MCP](../frontends/mcp.md#compilation-over-mcp), with the same semantics as
`jazyk compile`.

## Build lifecycle

One build, from `jazyk compile` to the verdict:

open the store → parse → align → dirty set → derive the board → sessions and commits,
re-deriving after each → checks → verdict

- Open the store for the build and take the build lease. A store of another version is
  archived and the graph starts empty. The first build is not special: it is
  reconciliation against an empty graph.
- Parse every document into its section tree ([parsing](./parsing.md)), align the trees
  across all documents ([alignment](./alignment.md)), and compute the
  [dirty set](./reconciler.md#dirty-set). A human save that dirtied sections journals an
  `edit` entry. Exact moves apply mechanically and journal an `align` entry. The rest
  become proposals for [`place-anchors`](./goals/place-anchors.md).
- Derive the board. `jazyk compile` prints the summary first:
  `compile: N goals (k kind, ...), b blocked`. An empty board skips straight to the
  checks.
- Loop while a goal is ready: the scheduler takes a batch, the harness assembles the
  [prompt](./sessions.md#the-prompt) (agent contract, skills, project block, goals block,
  loaded set), the session runs over ACP, and `done` runs the batch gates. The changeset
  commits: mutations applied, derived data recomputed, diagrams rendered, journal entry
  written with its resolved and opened goals, change records updated. The board
  re-derives. Goals a commit opened join the running session when they fit its locality
  and budget, or wait for a later one.
- As each cone's compile goals settle, its GC goals become ready and run in a burst
  (`gc burst: <kind> <target> (<count> > <limit>)`). A GC commit can reopen compile
  goals; the loop runs compile for that cone and comes back, bounded by flip detection
  and budgets.
- When no goal is ready, the deterministic [checks](./compilation.md#checks) run,
  [docsgen](../consumers/docsgen.md) renders, and the
  [verdict](./compilation.md#convergence) closes the build: `converged` with its
  `blocked` and `optional` counts, or `incomplete` with open, failed, blocked, and
  optional counts. Budget exhaustion parks the leftovers under an `incomplete-build`
  diagnostic; the next build resumes them first.

[Compilation](./compilation.md#a-build) states the sequence in full, with the
[edit paths](./compilation.md#edit-paths) that feed it and the
[incremental](./compilation.md#incremental-builds) behavior that makes a no-op rebuild
free.

## Outputs

Everything lives in the out directory (default `jazyk-out/`). See
[storage layout](./graph.md#storage-layout).

- `graph/`: the semantic graph, the primary output. Authored shards `entities.yaml`,
  `requirements.yaml`, `views.yaml`, `diagnostics.yaml`, `redirects.yaml`. Derived
  shards `relationships.yaml` and `state-machines.yaml`, rewritten on every commit.
- `docs/`: section trees and coverage per document.
- `diagrams/`: one `.puml` and one `.svg` per view under `<kind>/<slug>`, `.png` beside
  them on request. Build output, never read back. See
  [output layout](./diagrams.md#output-layout).
- `docsgen/`: one human-readable requirements document per entity, its diagrams
  embedded, rendered deterministically on every build. See
  [documentation generation](../consumers/docsgen.md#the-requirements-document).
- `gen/`: generation and verification metadata: the
  [ledger](../consumers/gen.md#the-ledger) and the criteria files for llm tests. The
  deliverable itself lives outside the out directory
  ([generation settings](./project-settings.md#generation)).
- `journal/`: one entry per generation, the audit trail of every change and the ground
  truth behind `jazyk ripple`.
- `trace/` and `sessions/`: session transcripts and the ACP session store.
- `status.yaml`: the store `version`, the generation, the verdict with its counts, the
  change records, parked and failed goals, costs per goal kind and class, and the
  alignment and re-evaluation blocks.
- `control.yaml`, `workers/`, `leases/`: the control plane. `feedback.jsonl`: what
  sessions reported through `report_feedback`. `.lock`: the store lock.

`jazyk check` exits non-zero when open diagnostics of severity `error` exist. See
[CLI](../frontends/cli.md#jazyk-check).
