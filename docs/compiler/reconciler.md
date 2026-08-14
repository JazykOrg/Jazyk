# Reconciler

The reconciler drives compilation. It compares the documents (desired state) against the
graph (observed state) and schedules [turns](./turns.md) until they agree. It is
deterministic code. The model never decides what is stale or what runs next.

The loop is level-triggered, not edge-triggered. A document change only enqueues work.
Every turn reads the current graph and the current documents, so a missed or duplicated
change notification is harmless. Initial compilation is not a special case: it is
reconciliation against an empty graph.

## Dirty set

Staleness is computed, never judged:

- [Parse](./parsing.md) every matched document into a section tree with per-section
  content hashes.
- Diff against the stored trees ([graph store, `docs/`](./graph.md#storage-layout)):
  - added or changed section → dirty,
  - removed section → dirty, plus the nodes anchored to it become stale anchors,
  - moved section (same hash, new reference) → not dirty; the store rewrites anchored
    references mechanically.
- Map dirty sections to affected graph nodes through mentions and requirement sources.

The work item for a document lists its dirty sections and stale anchors, so the turn sees
exactly what changed.

## Scheduling

- Granularity: one `reconcile-doc` turn covers all dirty sections of one document. If the
  dirty content exceeds roughly 60% of the context budget, the item splits by top-level
  heading groups.
- Order: breadth-first levels over the document link graph, starting from the
  [roots](./project-settings.md). The root document runs alone first, so the core
  vocabulary exists before anything else asks for it. Then its children run in parallel,
  then the next level. Documents unreachable by links run last, in path order.
- Parallelism within a level is bounded by the concurrency limit. Parallel turns are safe:
  commits serialize, and the store reconciles same-name creates at commit time. See
  [changesets](./graph.md#changesets). Duplicates under different names are repaired in
  the review wave, like any other duplicate.

## Waves

A build runs in waves:

- Ingest: `reconcile-doc` turns over the dirty documents, level by level.
- Fix-up, once per build, before judgment: documents holding sections still
  unprocessed, or requirements whose `quote` no longer locates, re-enqueue once.
  Coverage outranks review when the budget is tight; a fix-up that no longer fits the
  turn budget parks instead of vanishing, so the verdict stays honest.
- Pair review: `review-requirement` turns for every requirement the ingest and fix-up
  commits created or revised. Revised means the `ears` changed, or the source `quote`
  changed in substance (normalized text, not punctuation): the document text under a
  statement changing is exactly the case where the statement must be re-judged, even
  when a turn kept the old wording. Dirtiness propagates from sections to
  requirements: a changed statement must be re-judged against the statements it
  overlaps, not just re-extracted. For each changed requirement the reconciler computes a neighbor set
  deterministically:
  - candidates are requirements sharing an entity with it,
  - each is scored by overlapping content tokens: statement tokens minus stop words
    and the shared entities' own name tokens, reduced to crude stems, so "reverses"
    meets "reverse" and "sorting" meets "sort",
  - neighbors sharing at least two content tokens qualify, best six by score.
  Open `contradiction` and `duplicate-requirement` diagnostics are sticky pairs: a
  changed requirement also re-enqueues every partner such a diagnostic ties it to, so
  editing one side of a known pair always re-judges the other.
  Deletion propagates the same way. A commit that deletes a requirement (a turn's
  `delete_requirement` or the store's own [GC](./graph.md#garbage-collection)) dirties
  every open judged diagnostic naming it as a subject:
  - subjects that still exist re-enqueue for review (requirements for pair review,
    entities for entity review), and the open diagnostic alone is reason enough: such
    a target schedules even when the neighbor computation finds nothing,
  - the review pack marks the deleted subject, and the turn either resolves the
    diagnostic or refiles it against the surviving statements,
  - a diagnostic left with no existing subjects at all is resolved by the store at
    commit, journaled; no turn is needed to bury it.
  Without this, resolving a contradiction by rewriting one side strands the
  diagnostic open forever: its deleted subject can never be re-judged, and the
  surviving subject alone never becomes dirty.
  Propagation is also level-triggered, not just commit-triggered: the deterministic
  tail sweeps every open judged diagnostic for subjects missing from the graph and
  applies the same settlement. A graph deleted into a stranded state (before this
  rule existed, or by hand edits to the out directory) heals at the next build or
  queue poll instead of staying wedged.
  A changed requirement
  with no neighbors, no sticky partner, and no open diagnostic naming it schedules
  nothing. When two changed
  requirements are each other's only neighbor, the pair is one task, carried by the
  smaller id: judging A against B is judging B against A, and completing the task
  completes both. The turn shows each pair
  side by side and requires one verdict per neighbor (duplicate, contradiction, or
  consistent); see [turns](./turns.md#task-types). Neighbor selection is the
  reconciler's, never the model's.
  Its reach is lexical by design: a contradiction expressible only through concrete
  example values (a test case whose expected output encodes a sort order the prose
  contradicts) shares no tokens with its opposite and schedules no pair. The entity
  review is the net for those: it sees the entity's whole statement set and files
  what pairwise overlap cannot see. The pair turn is not gagged either: a verdict is
  owed only for the pairs shown, but a contradiction or duplicate the turn finds
  against a statement the pack did not pair may be filed with `report_diagnostic`
  all the same, provided the evidence is in quotes the turn has read.
- Review: `review-entity` turns for every entity whose fact set changed. Entities that
  share requirements or relationships form one review group; groups run in parallel,
  entities within a group run in order, so a judgment sees the merges and diagnostics of
  its neighbors. Whole groups run while they fit the turn budget; the rest parks and
  the next build resumes it.
- Checks: deterministic lint over the whole graph. Uncovered sections, unresolved stale
  anchors, entities with no requirements, unreachable entities from the declared roots
  (reachability follows relationships and shared requirements), and flip detection (a
  natural key deleted and recreated across recent builds becomes an
  `unstable-extraction` diagnostic).
- Document-quality checks, in the same wave: prose problems a human can fix, surfaced
  where the human writes ([LSP](../frontends/lsp.md) shows them inline). A section whose
  body exceeds the configured size (`section-too-large`), a document with too many
  sections (`doc-too-large`), an entity whose requirement count approaches the
  generation ceiling (`entity-too-dense`, the signal to split the topic into
  subsections), a matched file with no content (`empty-file`), and a relative link to
  a `.md` file whose target does not exist (`broken-link`). Turns never see these: an empty
  file has no dirty sections to schedule, and links only feed scheduling, so both
  problems are invisible to the model. The deterministic checks own them.
  Thresholds live in [limits](./project-settings.md#limits).
- Pinned-fact drift, when the [ledger](../consumers/gen.md#the-ledger) exists: a
  code-span literal in a requirement's statement that looks pinned (a path, an
  identifier, a value: it carries a digit, dot, slash, dash, colon, or underscore)
  and appears in none of the requirement's bound files becomes a
  `pinned-fact-drift` warning on that requirement. The docs say `us-east-1` and the
  code never mentions it: one of them is wrong, and no model is needed to notice.
  The diagnostic carries a [prompt](./model/diagnostic.md#prompts) (the docs are
  right and the code must change, or the value is stale here, or a freeform reply),
  and a human answer sticks: the check never re-asks a question a person already
  answered.

## The task queue

The reconciler's schedule is durable, derived state, not a private plan inside one
`compile` invocation. Any process computes the same queue from the same inputs: the
docs on disk, the graph, the ledger, and `status.yaml`. That is what lets an external
agent perform compilation over [MCP](../frontends/mcp.md#compilation-over-mcp) with the
same semantics as `jazyk compile`, and lets an interrupted build resume from any
consumer.

Task kinds, in dependency order:

- `reconcile-document`: derived from the section-tree diff, uncovered sections, and
  stale anchors. Ready when every document in an earlier
  [level](#scheduling) is clean.
- `review-requirement`: a changed requirement judged against its computed neighbors.
  Ready when no reconcile task is pending.
- `review-entity`: an entity whose facts changed. Ready when no reconcile or
  pair-review task is pending.
- `bind-requirement`: a requirement whose [binding](../consumers/bind.md) is absent
  or invalid (no ledger row, a reworded statement, a gone test artifact). Ready when
  the compile queue is empty: the statement must be final before a test encodes it.
- `generate-entity` and `verify-requirement`: [generation](../consumers/gen.md) and
  verification pending, derived from the ledger. Generation is ready when the compile
  queue is empty and none of the entity's requirements owes a bind; a row's
  verification is ready when its entity is generated.
- `draft-document`: [decompilation](../consumers/decompile.md), derived from the
  [unclaimed report](../consumers/bind.md#the-unclaimed-report). Always gated until a
  decompile release names its scope; there is no auto mode.

Everything above is derivable from disk except which reviews are owed: the ingest
turns that made an entity's facts change may have run in another process. So the
commit records it. Every committed changeset from a reconcile task adds its touched
entities and changed requirements to a `pending` block in `status.yaml`; a completed
review task removes its target. Review tasks then derive from `pending` exactly as
dirty sections derive from the section-tree diff. Parked work items persist beside it
as before, and resume first.

A consumer that just takes the first ready task and finishes it walks the same path
the internal loop walks: roots before the documents they link to, ingest before
pair review, pair review before entity review, reviews before generation. When the
last compile task finishes, the deterministic tail runs (checks, docsgen, verdict);
it needs no model, so whichever consumer emptied the queue runs it.

## The control plane

The queue says what work exists. The control plane says whether anyone may act on it
and who is acting. It is one file plus two directories in the out directory, so every
consumer (the internal loop, the GUI, an agent over MCP, `jazyk monitor`) reads the
same intent the same way the queue is the same everywhere. Without it, each frontend
invents its own policy in process memory and workers fight.

### Modes and releases

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

### Workers and leases

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

### Dispatch

`worker` in `control.yaml` (`internal`, `agent`, or `any`, default `any`) resolves
who acts on a release from the GUI:

- `agent`: the GUI records the release and stops; the attached agent's watcher does
  the work. No agent registered means the GUI says so and offers the internal run.
- `internal`: the GUI runs its own job, as today.
- `any`: prefer a live registered agent, fall back to internal. Leases make the race
  harmless either way.

## Convergence

The build is done when:

- a full wave proposes zero mutations (a fixed point),
- and the checks pass or their findings are recorded as diagnostics.

A hard per-build turn budget backstops the loop. Work still open when the budget runs out
is parked in `status.yaml` and reported as an `incomplete-build` diagnostic. The next
build resumes parked items first. Unfinished work is never silent.

The verdict in `status.yaml` is `converged` only when nothing is parked, no review is
pending in the [task queue](#the-task-queue), and no section with a body of its own is
left unprocessed. The verdict speaks to work completion, never to document health: a
graph can converge with open `error` diagnostics standing. So the verdict never
travels alone. Open diagnostic counts by severity (suppressed excluded) ride beside
it: in `status.yaml` (`diagnostics`), in the zero-task `compilation_tasks` reply, in
the final `done` reply, and in `await_changes`
(`openDiagnostics`). An agent deciding "done" sees the open errors in the same
breath as `converged`. A turn that exhausts its round budget commits
what it staged and reports no failure, so its document is not parked; counting only
parked items would report a build that stopped halfway as converged. Coverage is the
other half of the criterion, so a build that ran out of road says `incomplete` and the
next build picks the sections up.

## Coverage

Every section carries a coverage state in the store:

- `unprocessed`: not yet reconciled.
- `covered`: the model claimed it; its content is reflected in the graph.
- `non-normative`: the model marked it as carrying no requirements (examples, prose,
  navigation). A `note` is required.

Coverage is the completeness meter of a build and part of its termination criterion.
Checks flag sections that stay `unprocessed`, and `non-normative` sections whose text
still looks normative (`suspicious-non-normative`). "Looks normative" is a cheap
deterministic signal: the body says `shall`, uses obligation verbs (supports, manages,
handles, provides, requires, allows, stores, can be performed, is responsible), or
holds definition-list bullets (`` - `name` - description ``).

## Incremental builds

There is no separate incremental mode:

- Nothing changed → empty dirty set → zero turns, zero LLM calls.
- A cosmetic edit → one `reconcile-doc` turn that stages no mutations → graph unchanged.
- A real edit → turns for that document and review turns for the touched entities. The
  rest of the graph is not visited.

Idempotence and convergence replace per-stage caching. The graph plus the dirty set is
the cache.
