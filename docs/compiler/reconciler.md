# Reconciler

The reconciler drives [compilation](./compilation.md). It compares the documents
(desired state) against the graph (observed state) and schedules [turns](./turns.md)
until they agree. It is deterministic code. The model never decides what is stale or
what runs next. What a build does with the scheduled work, wave by wave, is
[compilation's](./compilation.md) to describe; whether anyone may act on it is the
[control plane's](./control-plane.md).

The loop is level-triggered, not edge-triggered. A document change only enqueues work.
Every turn reads the current graph and the current documents, so a missed or duplicated
change notification is harmless. Initial compilation is not a special case: it is
reconciliation against an empty graph.

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
    [`align-doc` turn](./turns/align-doc.md), and anchors with no candidate become
    stale anchors.
- Anchors the align turn placed with `reevaluate: true`, and anchors whose quote no
  longer locates, join the stale anchors of their document.
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

## The task queue

The reconciler's schedule is durable, derived state, not a private plan inside one
`compile` invocation. Any process computes the same queue from the same inputs: the
docs on disk, the graph, the ledger, and `status.yaml`. That is what lets an external
agent perform compilation over [MCP](../frontends/mcp.md#compilation-over-mcp) with the
same semantics as `jazyk compile`, and lets an interrupted build resume from any
consumer.

Task kinds, in dependency order (each links to the page stating exactly what the
model sees when the task runs):

- [`align-document`](./turns/align-doc.md): derived from the pending
  [alignment proposals](./alignment.md#what-applies-and-what-is-proposed) of one
  document. Ready when every document in an earlier [level](#scheduling) is clean.
- [`reconcile-document`](./turns/reconcile-doc.md): derived from the section-tree
  diff, uncovered sections, and stale anchors. Ready when every document in an
  earlier [level](#scheduling) is clean and the document has no pending alignment.
- [`review-requirement`](./turns/review-requirement.md): a changed requirement
  judged against its computed neighbors. Ready when no reconcile task is pending.
- [`review-entity`](./turns/review-entity.md): an entity whose facts changed. Ready
  when no reconcile or pair-review task is pending.
- [`bind-requirement`](./turns/bind-requirement.md): a requirement whose
  [binding](../consumers/bind.md) is absent or invalid (no ledger row, a reworded
  statement, a gone test artifact). Ready when the compile queue is empty: the
  statement must be final before a test encodes it.
- [`generate-entity`](./turns/generate-entity.md) and
  [`verify-requirement`](./turns/verify-requirement.md):
  [generation](../consumers/gen.md) and verification pending, derived from the
  ledger. Generation is ready when the compile queue is empty and none of the
  entity's requirements owes a bind; a row's verification is ready when its entity
  is generated.
- [`draft-document`](./turns/draft-document.md): [decompilation](../consumers/decompile.md),
  derived from the [unclaimed report](../consumers/bind.md#the-unclaimed-report).
  Always gated until a decompile release names its scope; there is no auto mode.

Everything above is derivable from disk except which reviews are owed: the ingest
turns that made an entity's facts change may have run in another process. So the
commit records it. Every committed changeset from a reconcile task adds its touched
entities and changed requirements to a `pending` block in `status.yaml`; a completed
review task removes its target. Review tasks then derive from `pending` exactly as
dirty sections derive from the section-tree diff. Parked work items persist beside it
as before, and resume first.

A consumer that just takes the first ready task and finishes it walks the same path
the internal loop walks: roots before the documents they link to, alignment before
ingest, ingest before
pair review, pair review before entity review, reviews before generation. When the
last compile task finishes, the deterministic tail runs (checks, docsgen, verdict);
it needs no model, so whichever consumer emptied the queue runs it.
