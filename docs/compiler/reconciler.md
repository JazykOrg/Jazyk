# Reconciler

The reconciler drives [compilation](./compilation.md). It compares the documents
(desired state) against the graph (observed state), derives the goals that separate the
two, and schedules [sessions](./sessions.md) until no mandatory goal remains. It is
deterministic code. The model never decides what is stale, what work exists, or what
runs next: it resolves, fails, or parks the goals it is handed
([division of labor](./compiler.md#division-of-labor)). What a build does with the
scheduled work is [compilation's](./compilation.md) to describe; whether anyone may act
on it is the [control plane's](./control-plane.md).

The loop is level-triggered, not edge-triggered. A document change only produces change
records. Every derivation reads the current documents, the current graph, the
[ledger](../consumers/gen.md#the-ledger), and `status.yaml`, so a missed or duplicated
change notification is harmless, any process computes the same board, and an
interrupted build resumes from any consumer. Initial compilation is not a special case:
it is reconciliation against an empty graph. A rebuild with nothing changed derives zero
goals and makes zero LLM calls.

## Dirty set

Staleness is computed, never judged:

- [Parse](./parsing.md) every matched document into a section tree with per-section
  content hashes.
- [Align](./alignment.md) the trees against the stored ones
  ([graph store, `docs/`](./graph.md#storage-layout)), across all documents:
  - added or changed section → dirty,
  - moved section (same title and body, new reference) → not dirty; the store
    rewrites anchored references mechanically,
  - edited, split, merged, or removed section → dirty where it still exists; its anchors
    are relocated to their best candidate as proposals for the
    [`place-anchors` goal](./goals/place-anchors.md), and anchors with no candidate
    become stale anchors.
- Anchors a `place-anchors` session placed with `reevaluate: true`, and anchors whose
  quote fails to locate, join the stale anchors of their document.
- Map dirty sections to affected graph nodes through mentions and requirement sources.

The dirty set is recorded, not held in memory. Two journal entries carry it, each
journaled when it has something to record, each with a fresh generation number (see
[journal](./graph.md#journal)), each writing one
[change record](./graph.md#change-records) per finding under its generation with
`mutation: 0`.

The `edit` entry records what a save did to the sections. A human save is a generation
like any commit, the root of every ripple. It writes:

- `section-dirty`: the subject is the section reference. `detail` carries the alignment
  outcome (`added`, `edited`, `split`, `merged`) and the previous hash.
- `section-removed`: the subject is the reference the section had. `detail` lists the
  nodes anchored there. It derives no goal of its own; the records that follow from it
  (`anchor-stale`, `node-deleted`, `view-member-gone`) carry the trail onward.

The `align` entry records what alignment did with the anchors: the exact moves it
applied mechanically, and the proposals and stale anchors it leaves for a session. It
writes:

- `anchor-stale`: the subject is the section reference the anchor last pointed to, even
  when that section is gone. `detail` lists the anchored nodes and why each is stale
  (`no-candidate`, `reevaluate`, `quote-missing`).
- `alignment-pending`: the subject is the document path. `detail` counts the proposals
  persisted under `alignment` in `status.yaml`
  ([what applies and what is proposed](./alignment.md#what-applies-and-what-is-proposed)).

Sections whose coverage is still `unprocessed` need no record:
[coverage](./compilation.md#coverage) is graph state, and `reconcile-section` goals
derive from it directly. A store whose version does not match the binary is archived
whole and the build starts from an empty graph
([storage layout](./graph.md#storage-layout)); that too is an ordinary dirty set, with
every section dirty.

## Goal derivation

Compilation is a goal board. A goal is one unit of work the harness holds evidence for:
a dirty section, a revised statement, a dangling view member, a crossed limit. The board
is derived, never stored. `derive_goals` recomputes it from disk whenever it is
consulted: at the start of a build, after every commit, on every `goals` call over
[MCP](../frontends/mcp.md#compilation-over-mcp), and for the
[GUI board](../frontends/gui.md#board). The inputs are the documents, the graph, the
ledger, and the change records, parked list, and failed list in `status.yaml`. The graph
never stores goals, so it cannot grow with them.

E.g.:

```yaml
g:retrace:view:usecase/holds:
  kind: retrace
  class: compile
  mandatory: true
  target: view:usecase/holds
  unit: view
  change: {deleted: req:orders-6, in: g409}
  cause: {generation: 409, mutation: 2, via: members}
  state: open
  hints:
    - load view:usecase/holds
    - skill flow-views
```

- `kind` names a row of the [catalog](#the-catalog). `class` is `compile` or `gc`.
- `mandatory` goals block convergence; optional goals advise.
- `target` is a node id, a section reference (`doc.md#/ref`), a document path, a pair
  (`req:a~req:b`, smaller id first), or `scope:<scope>` (the top level of a scope, its
  parentless entities; `scope:public` for the default scope, see
  [the scope root](./concepts/levels.md#the-scope-root)). The goal id is
  `g:<kind>:<target>`.
- `change` is the attached evidence and the goal's identity: re-deriving the board
  matches a goal to its predecessor by the change, so a goal survives across builds and
  processes without being stored.
- `cause` names the committed change that spawned the goal: the generation, the
  mutation index within it (`0` for store-level causes), and the edge or computation
  that carried the dirtiness.
- `state` is `open`, `blocked {on}` (a human answer, a ratification, a release),
  `parked`, or `failed {reason}`.
- `hints` are computed and honest: what to load, which skill explains the shape, which
  tool typically resolves the kind. They are suggestions; the gate is the truth.

### The registry

One Rust trait, one static registry compiled into the binary, one implementation per
kind. The trait surface is small because the store, gates, context engine, journal,
trace, and control plane are generic underneath it:

- `kind`: the name. `class`: compile or GC. `unit`: what one target is (document,
  section, pair, entity, node, requirement, ledger row, view), so the board and the GUI
  can render it.
- `derive_goals(store, status) -> Vec<Goal>`: this kind's goals from disk state.
  Deterministic, idempotent, cheap.
- `ready(goal, board) -> Ready | Blocked(reason)`: the [readiness](#readiness) rule,
  the reason rendered as a sentence because every surface shows it.
- `pack(store, batch)`: a batch's initially [loaded set](./context.md#the-loaded-set).
- `toolset()`: the kind's slice of the one [tool registry](./tools.md#toolsets).
- `gates(changeset)`: the batch gate `mark_goal_done` and `done` validate, on top of the
  store's [per-mutation gates](./graph.md#validation-gates).
- `prompt`: the contract paragraph, a payload file under `goals/prompts/` embedded at
  compile time ([the prompt](./sessions.md#the-prompt)).

Adding a capability is one module: a kind, its gate, its hint computer, its skill. No
tool enqueues work: the model writes graph state, the harness derives the goals.

### Change records

Current graph state alone cannot say "revised since last judged"; the change records
can. Every commit writes the typed dirtiness its mutations caused into `status.yaml`
`changes` ([change records](./graph.md#change-records)): which sections changed, which
requirements were created, revised, or deleted, which entities' fact sets changed,
which thresholds crossed. The writers are the dirty set computation (the `edit` and
`align` generations), the store at commit (from the changeset's mutations and the
[derived data](./graph.md#derived-data) recompute), the deterministic sweep
([garbage collection](./graph.md#garbage-collection)), the ledger comparison at
derivation, and the [checks](./compilation.md#checks).

- One record per kind and subject. A later commit that causes the same kind on the same
  subject supersedes the record: the `cause` moves to the new commit and `detail`
  merges.
- `derive_goals` reads a goal's `change` from exactly its record. Several records on one
  target with the same goal kind fold into one goal whose `change` merges their details
  and whose `cause` is the earliest of them.
- Resolving a goal clears its records: the commit that carries an accepted
  `mark_goal_done` deletes them, and the goal is gone. A goal whose record is gone is
  gone.
- The sweep at commit drops records whose subject does not exist, except the trail
  kinds (`section-removed`, `requirement-deleted`, `entity-deleted`), whose subject is
  the dead node by design.
- A record whose evidence lapses on its own clears without a session: a
  `threshold-crossed` record clears when the count falls back under its threshold, a
  `ledger-stale` record when the row is current, an `edges-missing` record when the
  requirement gains edges by any path.

The record kinds and what derives from each:

| record kind | subject | written by | derives |
|---|---|---|---|
| `alignment-pending` | document | dirty set (the `align` entry) | `place-anchors` |
| `section-dirty` | section reference | dirty set (the `edit` entry) | `reconcile-section` |
| `anchor-stale` | section reference | dirty set (the `align` entry) | `reconcile-section` |
| `section-removed` | section reference | dirty set (the `edit` entry) | nothing (trail) |
| `requirement-created`, `requirement-revised` | requirement | commit | `rejudge-pair` per [neighbor](#pairs) |
| `requirement-deleted`, `entity-deleted` | the dead node | commit, sweep | nothing (trail; the ledger prunes on it) |
| `node-deleted` | a live node that referenced the dead one | sweep | `retrace` (view, instance, derived fact); `rejudge-pair` (a surviving diagnostic subject) |
| `entity-changed` | entity | commit, sweep | `review-entity` |
| `instance-changed` | instance entity | commit | `conform-instance` |
| `view-member-gone` | view | sweep | `retrace` |
| `ledger-stale` | requirement (`bind`, `verify`), entity (`generate`) | the ledger comparison at derivation, `detail.goal` naming the kind | `bind`, `generate`, `verify` by `detail.goal` |
| `prompt-unanswered` | diagnostic | commit | `answer` |
| `provenance-pending` | derived or decreed node | commit | `ratify` |
| `threshold-crossed` | the node over a limit | commit (recompute) | `split-view`, `abstract-entity` |
| `edges-missing` | requirement | commit | `declare-edges` |
| `lookalike` | entity pair | commit (name index) | `dedupe-candidates` |
| `query-match`, `flow-unplaced` | view | commit, checks | `curate-view` |

`via` names how dirtiness reached the subject. The set is closed here, and every goal
page draws its values from it:

- a stored reference: `section`, `quote`, `mentions`, `entities`, `edges`,
  `transition`, `parent`, `members`, `excluded`, `collapse`, `from`, `subjects`,
  `attributes` (an attribute's value, type, or provenance quote);
- a change on the node itself: `fields` (an entity's own fields changed, or the entity
  was created), `merge` (another entity was merged into it), `provenance` (a derived
  or decreed fact landed);
- a computation: `alignment`, `ledger`, `limits`, `lookalike`, `query`,
  `flow-placement`, `recompute` (a derived relationship added, removed, or retyped),
  `sweep`, or the rule name of the check that wrote the record;
- the tool that filed a `prompt-unanswered` record: `report_diagnostic`,
  `update_diagnostic`, `record_generation`.

The ledger goals share one record kind. The ledger comparison at derivation writes a
`ledger-stale` record wherever the ledger and the graph disagree, `detail.goal` naming
the kind and `detail.reason` the
[derived status](../consumers/gen.md#status-is-derived-never-stored) behind it:

- `bind` on a requirement with no current row (`unbound`, `requirement-changed`,
  `artifact-gone`).
- `generate` on an entity whose facts differ from the ledger (`facts-changed`) or one
  of whose requirements has an `unimplemented` row: the requirement is bound and
  nothing implements it, so the bound test is the acceptance gate
  ([the cascade](../consumers/gen.md#the-cascade)).
- `verify` on a requirement whose row says action (`never-run`, `test-changed`,
  `code-changed`, `runner-failed`).

A `failing` row (a `fail` verdict on implementing files) is a finding for the author,
a diagnostic and never a goal. `reconcile-section` alone derives from state beside the
records: every section with a body of its own whose coverage is `unprocessed`.

### Pairs

A `rejudge-pair` goal targets one pair of requirements. Neighbor selection is the
reconciler's, never the model's. For each requirement with a `requirement-created` or
`requirement-revised` record (revised means the `statement` changed, or the source
`quote` changed in substance: normalized text, not punctuation), the neighbor set is
computed deterministically:

- candidates are requirements sharing an entity with it,
- each is scored by overlapping content tokens (statement tokens minus stop words and
  the shared entities' own name tokens, reduced to crude stems, so "reverses" meets
  "reverse" and "sorting" meets "sort") plus the count of shared entities,
- neighbors sharing at least two content tokens qualify, and so do neighbors sharing
  at least two entities: a restatement built from the same entities can share every
  noun and still share no other token, because the shared names leave the token pool.
  Best six by score.

Open `contradiction` and `duplicate-requirement` diagnostics are sticky pairs: a changed
requirement also pairs with every partner such a diagnostic ties it to, so editing one
side of a known pair always re-judges the other. Deletion propagates the same way: a
`node-deleted` record on a surviving subject of an open judged diagnostic derives a
`rejudge-pair` goal for it, and that open diagnostic alone is reason enough. The session
sees the deleted subject marked and either resolves the diagnostic or refiles it against
the surviving statements. A diagnostic left with no existing subjects at all is resolved
by the store at commit, journaled as `settle-diagnostics`; no session is needed to bury
it. The sweep is level-triggered: the [checks](./compilation.md#checks) settle every
stranded diagnostic the same way, so a store deleted into a stranded state by hand edits
heals at the next build.

A changed requirement with no neighbor, no sticky partner, and no open diagnostic
naming it derives no pair goal. A pair is one goal, `g:rejudge-pair:req:a~req:b`,
carried by the smaller id: judging A against B is judging B against A, so two changed
requirements that neighbor each other derive one goal, not two. The reach is lexical by
design: a contradiction expressible only through concrete example values shares no
tokens with its opposite and derives no pair. `review-entity` is the net for those; it
sees the entity's whole statement set.

### Fan-out

An [`abstract-entity`](./goals/abstract-entity.md) goal derives on a node whose direct
children exceed the `children-per-entity` limit, or on the scope root when its
parentless entities do ([levels](./concepts/levels.md#levels),
[the scope root](./concepts/levels.md#the-scope-root)). This is the fan-out variant of
the goal, beside the caps variant on requirement and state counts. The harness counts
and computes coupling; the model names and judges.

- The record is `threshold-crossed` with `detail.limit: children-per-entity`, written
  by the limit counts at commit ([the registry](./graph.md#the-registry): soft 9, hard
  15). Its subject is the node, or `scope:<scope>` for the scope root; `via` is
  `limits`. It is level-triggered like every threshold record: crossing soft derives
  the goal optional, hard makes it mandatory at every derivation, and a count back
  under soft clears the record without a session ([escalation](#escalation)).
- The goal is `g:abstract-entity:<ent>` or `g:abstract-entity:scope:<scope>`. Its
  `change` is `{fan_out: n, limit: {soft, hard}, candidates: [[id, ...], ...]}`: the
  count, the thresholds in force, and the coupling partitions the hint computer
  proposes. A record for another limit on the same node folds into the same goal, as
  records do ([change records](#change-records)).
- The cone is the node's subtree, the downward walk from the node ([cones](#cones)).
  The root form's cone is the whole scope. The goal is ready under the GC rule, when no
  compile goal is open or parked in the cone ([GC gating](#gc-gating)), so a level is
  regrouped once, over settled children.
- Hints: the fan-out count, the candidate partitions with their cohesion scores, the
  members' stereotypes, the document each member is mentioned in most (documents and
  headings are strong naming hints, see [naming](./concepts/levels.md#naming)), and
  any existing grouping under the node ([groupings](./concepts/levels.md#groupings)).

The hint computer (the `abstract-entity` kind's, in the [registry](#the-registry))
works over the target's direct children:

- `weight(a, b)` is the number of requirements referencing both `a` and `b` plus the
  number of derived relationships between them. Descendants count: a requirement or a
  relationship on a descendant lifts to the child it sits under, so a leaf's edges reach
  the level.
- Greedy agglomeration: start with every child as a singleton cluster; repeatedly merge
  the pair of clusters with the highest total weight between them (the sum of `weight`
  over their member pairs); stop when the cluster count is at or under the soft
  threshold and every cluster has at least two members or is a singleton no other
  cluster touches (zero weight to every other cluster).
- Ties break by id: among merges of equal weight, the pair whose ids sort first.
- Output: each cluster as an ordered id list with its internal weight, largest first,
  capped at the soft threshold of clusters and at 12 ids per cluster (the rest
  summarized as a count).
- Deterministic: a re-derivation over the same graph yields the same candidates, so the
  goal's `change` is stable and matches its predecessor across builds and processes.

The candidates are suggestions; the gate is the truth. The model may accept a
candidate, adjust it with reasons, or decline it with a reason. The harness never names
a grouping, and the model never counts. A grouping lands through `group_entities` and
undoes through `dissolve_entity` ([write tools](./tools.md#write-tools)); a level that
is genuinely flat fails the goal with that reason rather than gaining invented tiers.

### The catalog

Compile goals bring the graph in line with the documents. GC goals restructure and tidy.
`M` mandatory, `O` optional, `O→M` optional until the hard threshold, `B` blocked on a
human. A kind derives only when its input exists in the graph; nothing enumerates
features. Each kind's page states its gate, its hints, its contract paragraph, and its
tools.

| kind | class | tier | m | unit | derives from |
|---|---|---|---|---|---|
| [`place-anchors`](./goals/place-anchors.md) | compile | 0 | M | document | `alignment-pending` |
| [`reconcile-section`](./goals/reconcile-section.md) | compile | 1 | M | section | `section-dirty`, `anchor-stale`, `unprocessed` coverage |
| [`rejudge-pair`](./goals/rejudge-pair.md) | compile | 2 | M | pair | `requirement-created`, `requirement-revised`, sticky pairs, `node-deleted` |
| [`review-entity`](./goals/review-entity.md) | compile | 2 | M | entity | `entity-changed` |
| [`retrace`](./goals/retrace.md) | compile | 2 | M | node | `view-member-gone`, `node-deleted` |
| [`conform-instance`](./goals/conform-instance.md) | compile | 2 | M | instance | `instance-changed` |
| [`bind`](./goals/bind.md) | compile | 3 | M | requirement | `ledger-stale` (`goal: bind`: no current row) |
| [`generate`](./goals/generate.md) | compile | 3 | M | entity | `ledger-stale` (`goal: generate`: facts differ from the ledger, `unimplemented` rows) |
| [`verify`](./goals/verify.md) | compile | 3 | M | ledger row | `ledger-stale` (`goal: verify`: the derived status says action) |
| [`ratify`](./goals/ratify.md) | compile | 3 | B | fact | `provenance-pending` |
| [`answer`](./goals/answer.md) | compile | 3 | B | diagnostic | `prompt-unanswered` |
| [`declare-edges`](./goals/declare-edges.md) | gc | | O | requirement | `edges-missing` |
| [`dedupe-candidates`](./goals/dedupe-candidates.md) | gc | | O | entity pair | `lookalike` |
| [`curate-view`](./goals/curate-view.md) | gc | | O | view | `query-match`, `flow-unplaced` |
| [`split-view`](./goals/split-view.md) | gc | | O→M | view | `threshold-crossed` (view limits) |
| [`abstract-entity`](./goals/abstract-entity.md) | gc | | O→M | entity | `threshold-crossed` (entity limits, state limit; the [fan-out](#fan-out) form on a node or on `scope:<scope>`) |

`retrace` is one kind: delete a requirement, and the flow view that stepped through it,
the instance that conformed to it, and the derived fact that cited it each surface as a
`retrace` goal with the same cause, each hinting what to load to see the damage. Derived
data needs no retrace: [relationships](./model/relationship.md#recompute), state
machines, and [default views](./model/view.md#default-views) recompute at commit.
Decompilation stays outside the board: draft work is released per scope
([decompilation](../consumers/decompile.md#triggering)).

## Scheduling

One build runs one loop, sequentially: one session at a time, each with one batch.

- Derive the board. Take the highest ready [tier](#readiness) that has an open goal;
  parked goals resume first.
- Group the ready goals by [locality](#batching) and fill one batch under the context
  budget.
- Run one session for the batch ([execution](./sessions.md#execution)). The session
  prompt lists exactly the batch's goals and one summary line for the rest.
- Commit the changeset: derived data recomputes, change records land, the journal entry
  records `resolved_goals` with their justifications and `opened_goals` with their causes
  ([commit](./sessions.md#commit)).
- Re-derive the board. GC goals whose cone the commit quieted are ready; they run next,
  in a burst, before the loop returns to the compile tiers, unless the build budget is
  tight, in which case compile outranks GC.
- Repeat until no goal is ready or a budget is spent. Then the deterministic tail runs:
  [checks](./compilation.md#checks), rendering, docsgen, the verdict. The tail needs no
  model, so whichever consumer emptied the board runs it.

A consumer that claims the next batch over MCP (`begin_goals`) and finishes it (`done`)
walks the same path the internal loop walks
([compilation over MCP](../frontends/mcp.md#compilation-over-mcp)). The trace shows each
step as `batchStart`, `sessionStart`, `goal`, and `gcBurst` events
([trace events](./sessions.md#trace-events)).

## Readiness

Ordering inside compile is small and internal: alignment before ingest, ingest before
judgment, judgment before ledger work. Tiers carry it. GC goals sit outside the tiers
and wait for their cone instead.

### Tiers

| tier | kinds | ready when |
|---|---|---|
| 0 | `place-anchors` | always |
| 1 | `reconcile-section` | no tier 0 goal is open or parked; the document's link level is reached; the document has no `alignment-pending` record |
| 2 | `rejudge-pair`, `review-entity`, `retrace`, `conform-instance` | no tier 0 or 1 goal is open or parked |
| 3 | `bind`, `generate`, `verify`, `ratify`, `answer` | no tier 0, 1, or 2 goal is open or parked |

- Open and parked goals hold the later tiers. Failed and blocked goals do not: a section
  the model could not reconcile, or a document awaiting a release, must not wedge every
  judgment behind it. They count in the [verdict](./compilation.md#convergence) instead.
- Within tier 3, `generate` for an entity also waits until none of its requirements
  owes a `bind` (the statement must be final before a test encodes it), and `verify`
  waits until the row's entity is generated. `ratify` and `answer` are never ready for
  a session: they are `blocked {on: human}` and resolve through the human paths their
  pages describe.
- A release gate ([modes and releases](./control-plane.md#modes-and-releases)) renders
  as `blocked {on: release}`.

### Link levels

Link levels order tier 1: breadth-first over the document link graph from the
[roots](./project-settings.md#roots).

- The root documents are level 0. A document's level is one more than the lowest level
  of any document linking to it.
- The root documents run first, so the core vocabulary exists before anything else asks
  for it; then their children; then the next level. A document's level is reached when
  every document in an earlier level has no open or parked tier 1 goal.
- Documents unreachable by links run last, in path order.

### GC gating

A GC goal is ready when no compile goal is open or parked in its target's
[cone](#cones). Blocked and failed compile goals do not count: a ratification proposal
that a GC goal itself created must not hold its own cone, and a section the model could
not reconcile is as settled as it will get. Restructuring therefore always sees settled
content: an entity is abstracted knowing every requirement this build gives it, never a
stream of partial states. Nothing waits for a global phase: as each cone settles, its GC
goals become ready and run right there, while the graph is loaded. Mandatory GC goals
run before optional ones within a burst.

### Cones

The cone of a target is the set of nodes reachable from it through stored references.
The references are the stored edges of the [model](./model.md#edge-summary): `parent`
(entity to its containing entity) and `parents` (section to section), `mentions`
(entity to section), `entities`, `edges`, and `transition` (requirement to entity),
`members` and `collapse` (view to node; a `query` counts as listing what it matches),
and `from` (derived provenance to upstream node). Every reference has a referrer and a
referent, and the walk keeps one direction at a time:

- Upward, transitively: every referent of the target, then every referent of those.
  For an entity that is its `parent` chain, its `mentions` sections and their `parents`
  chains, and its `from` nodes. For a requirement it is its entities, their parents, and
  its `from` nodes.
- Downward, transitively: every referrer of the target, then every referrer of those.
  For an entity that is its descendants over `parent`, the requirements naming it or any
  descendant in `entities`, `edges`, or `transition`, the views listing any of them in
  `members` or `collapse`, and every node whose `from` names any of them.
- Never sideways: a node reached upward contributes no referrers, and a node reached
  downward contributes no referents. Without this rule the cone of one entity would be
  its whole connected component.

A compile goal is in the cone when its target is a node in the cone, when its target is
a pair with a member in the cone, when its target section anchors a node in the cone (a
requirement `source` or an entity mention points into that section), or when its target
document contains such a section.

The cone of a leaf entity is its requirements, its views, the derived facts built on it,
and the sections that anchor them. The cone of a top-level «system» entity is most of
the graph, which is correct: abstracting the top of the tree must wait for everything
under it. The target `scope:<scope>` names the scope root
([the scope root](./concepts/levels.md#the-scope-root)); its cone is the downward walk
from every parentless entity of the scope, which is the whole scope, so the top level
is regrouped only when everything in the scope has settled ([fan-out](#fan-out)). The
same computation answers `jazyk explain <target>`: the goals a change to the target
would open ([`jazyk explain`](../frontends/cli.md#jazyk-explain)).

## Batching

The model never sees the whole board. Each session gets one batch. The scheduler forms
a batch from goals of one class and one tier that resolve to one
[executor](./control-plane.md#executors), grouped by locality:

- document locality: `place-anchors` is one goal for the document; `reconcile-section`
  goals of one document batch in document order, adjacent sections together.
- node locality: `rejudge-pair`, `review-entity`, `retrace`, and `conform-instance`
  goals whose targets share an entity, a requirement, or a view batch together, so a
  judgment sees the merges and diagnostics of its neighbors. Entities that share
  requirements or relationships form one group.
- view locality: view goals batch with the goals on the view's members.
- level locality: an `abstract-entity` goal on a node or on `scope:<scope>` has node
  locality through the level's members, the direct children or the scope's parentless
  entities ([fan-out](#fan-out)).
- ledger locality: `bind`, `generate`, and `verify` goals join through their entity's
  component group root, so the ready goals of one component subtree form one batch
  ([grouping by component](../consumers/gen.md#grouping-by-component)). A flat graph
  has no groups, and the ledger goals batch per entity.

The batch fills until the budget says stop. The skills of the batch's goal kinds render
into the same session context budget (24000 chars, a
[registry constant](./graph.md#limits)), so the registry's `pack` computes the batch's
initially loaded set from the goals' hints under what the skill payloads leave; a goal
whose loads do not fit waits for the next batch. The initial set never passes the
high-water mark: a goal's own target always loads full, and a supporting item that
would land the set past the mark enters as a stub instead
([policy](./context.md#policy)). The round budget bounds the count too: at least 8 of the session's
24 rounds per section in the batch, so a reconcile batch holds at most three sections.
A batch is one to a handful of goals; the count is a consequence of budget and locality,
never a fixed number. Parked goals are batched first.

Formed is not the same as carried. A batch is formed of one class and one tier; the
session that runs it may resolve more. A previewed goal of either class joins the
running session under the join conditions of [bubbling](#bubbling) (it lies in the
batch's locality, the session's toolset covers its kind, its kind resolves to the same
executor, the remaining budget fits it), so a compile session can end by resolving the
GC goal its own commit made ready.

A batch has an id, `b<generation>-<n>`: the generation the board derives from and the
batch's index within it. A re-derivation of the board renames its batches, so an id
never outlives the board it came from. It names the session, its lease
([workers and leases](./control-plane.md#workers-and-leases)), its trace file, and the
`{target}` of the worker protocol line. The toolset of the session is the union of the
batch's kinds' toolsets ([toolsets](./tools.md#toolsets)); the skills are the union of
their skills ([skills](./sessions.md#skills)). `jazyk preview` renders the next batch's
prompt exactly as the model would receive it
([preview](./sessions.md#preview)).

## Bubbling

Downstream work is never silent and never model-invented. Staged mutations are validated
when staged ([staged mutations](./sessions.md#staged-mutations)), and the same
computation previews what the mutation will open and what it will make ready. The tool
reply says so. E.g.:

```
ok: delete_requirement req:orders-6 staged
this delete will open: retrace view:usecase/holds (member gone), retrace ent:order (statement gone)
```

- At commit the previews become real goals with causes, recorded as `opened_goals` in
  the journal entry, and the trace prints a `goal` event per goal
  ([trace events](./sessions.md#trace-events)). `jazyk watch` prints one line per goal
  opened or resolved.
- A previewed goal joins the running session when it lies in the batch's locality, the
  session's toolset covers its kind, its kind resolves to the session's executor, and
  the remaining budget fits it: the reply lists it as available, the session resolves it
  with `mark_goal_done` like any goal in the batch, and the commit records it as opened
  and resolved in one generation. This is how a GC burst often lands in the session that
  just settled the cone: the staged resolution of the cone's last compile goal previews
  the GC goal as ready.
- Everything else waits for a later batch. A previewed goal that never materializes
  (the mutation was dropped before `done`) opens nothing.

## Escalation

Every [limit](./graph.md#limits) carries a soft and a hard threshold. Crossing the soft
threshold writes a `threshold-crossed` record and derives an optional goal (`split-view`
for view limits, `abstract-entity` for entity limits and the state limit, the
`children-per-entity` row on a node or on `scope:<scope>` included, see
[fan-out](#fan-out)). Crossing the
hard threshold escalates the same goal to mandatory: `mandatory` is recomputed at every
derivation from the current count, so a count that drops back under the hard threshold
de-escalates, and one that drops under the soft threshold clears the record without a
session. The build cannot report `converged` while a mandatory limit goal is open or
failed; the diagram renders meanwhile with collapse applied and marked as such
([over-limit views](./diagrams.md#over-limit-views)).

Dismissing a limit goal is a graph write, not goal state. The node's own limit is
raised (`limits: {<limit>: n}`) with decree provenance recorded in the journal
(`kind: decree`); the goal derives only when the count crosses `n`, and escalates when
it crosses `n` plus the registry's distance between soft and hard. Nothing tunes limits
in `jazyk.toml`; the registry is built into the binary, and bumps are per node.

The other escalations are states, not thresholds: a mandatory goal a session fails
blocks convergence ([parked and failed](#parked-and-failed)); a goal that waits on a
human is `blocked` and counted; a build that exhausts its cap parks what is open and
files `incomplete-build`. A dead endpoint trips a breaker before the cap: five
consecutive failed sessions that spent no tokens park what is open the same way, the
last error named in the reason, because an endpoint answering only errors (a rate
limit, an outage) must not grind the session cap into hundreds of futile attempts.

## Parked and failed

`status.yaml` carries both lists beside the change records: `parked` (whole goal
records) and `failed` (`{goal, reason}`, `goal` the whole record). Both are read at
derivation and stamp the goal's `state`. Both keep the goal whole, `change` payload
included, so a parked or failed goal survives a re-derivation that would otherwise drop
it.

- Parked means "ran out of road": a session that exhausted its rounds without meeting
  the gate is retried once with a fresh session, then its goals park; a build that hits
  its cap (3 × derived goals + 8 sessions) parks every open goal. Parked goals keep
  their records, resume first in the next build, and count as open in the verdict
  (`incomplete`, with an `incomplete-build` diagnostic). Unfinished work is never
  silent.
- Failed means the session said so: `mark_goal_failed({goal, reason})` is always
  available, because a goal that cannot be accomplished (documents too deeply
  contradictory, a target that makes no sense any more) must be failable, or the board
  fills with dishonestly resolved goals. A failed goal keeps its target, so the failure
  surfaces on the thing itself everywhere it renders. A failed mandatory goal blocks
  convergence; a failed optional goal is recorded and stands.
- A failed goal never re-runs by itself. It reopens when its subject changes again (a
  new change record on the subject drops the `failed` entry at commit) or when a human
  retries it from the board or removes its entry from `failed`. `jazyk explain <goal>`
  shows the reason.
- Resolving a parked or failed goal clears its entry with its record.

## Flip detection

Oscillation is caught on natural keys, never on ids. No dedicated key history is
stored: the journal is the history, and tombstone redirects mark deletions
([garbage collection](./graph.md#garbage-collection)).

- Recreation within one class is caught on tombstoned slugs. A deleted entity leaves a
  tombstone redirect on its slug, so recreating the natural key mints the new id with a
  collision suffix. An entity id carrying a collision suffix while a tombstone holds
  the base slug files `unstable-extraction` ([checks](./compilation.md#checks)).
- A flip between the classes is a GC commit undoing what a compile commit established on
  a key, or the reverse: `abstract-entity` introduces `ent:order-pricing` with derived
  provenance, `review-entity` merges it back into `ent:order`, `abstract-entity` derives
  again and re-creates it. The check replays the journal's `session` entries into a
  per-key event list: entity creates, deletes, merges, and decree retractions, each
  stamped with its generation, the class of the session's resolved goal, and that
  goal's justification from `resolved_goals`. Two flips of one natural key between the
  classes park the pair: both goals move to `parked` and stay there, and one
  `unstable-derivation` diagnostic is filed on the key's surviving node with both
  justifications side by side (the latest from each direction). The diagnostic carries a
  [prompt](./model/diagnostic.md#prompts): keep the split, keep the merge, or a
  freeform ruling. The pair is blocked on that answer; answering it clears the parked
  entries and the next build resumes the chosen direction.
- A reparent flip is a child that moves between the same two parents across
  generations: `ent:cache` moves from `ent:backend` under `ent:storage` in one
  generation (a `group_entities`, a `dissolve_entity`, an `update_entity` or
  `edit_fact` on `parent`, or the sweep's dissolve) and back in a later one. The check
  replays every journaled `parent` change, each mutation carrying the prior parent
  ([journal](./graph.md#journal)), into a per-child event list keyed on the child's
  natural key with the two parents matched by natural key too, so a grouping dissolved
  and re-minted under a new id counts as the same parent. The second move parks like a
  cross-class flip: the goal behind it moves to `parked` and stays there, and one
  diagnostic is filed on the child with both justifications side by side and a
  [prompt](./model/diagnostic.md#prompts): keep it under the first parent, keep it
  under the second, or a freeform ruling. Answering it clears the parked entry and the
  next build resumes the chosen direction.

Flip detection and the budgets bound the alternation between the classes and the
alternation of a child between parents. With them,
idempotence makes convergence a fixed point rather than a loop: a session that re-derives
an unchanged conclusion stages a no-op upsert, no mutation lands, no record is written,
and that branch of the cascade dies.
