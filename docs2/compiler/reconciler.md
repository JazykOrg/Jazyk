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
- Review: `review-entity` turns for every entity whose fact set changed. Entities that
  share requirements or relationships form one review group; groups run in parallel,
  entities within a group run in order, so a judgment sees the merges and diagnostics of
  its neighbors.
- Checks: deterministic lint over the whole graph. Uncovered sections, unresolved stale
  anchors, entities with no requirements, unreachable entities from the declared roots
  (reachability follows relationships and shared requirements), and flip detection (a
  natural key deleted and recreated across recent builds becomes an
  `unstable-extraction` diagnostic). Findings may enqueue one bounded fix-up pass.
- Document-quality checks, in the same wave: prose problems a human can fix, surfaced
  where the human writes ([LSP](../frontends/lsp.md) shows them inline). A section whose
  body exceeds the configured size (`section-too-large`), a document with too many
  sections (`doc-too-large`), and an entity whose requirement count approaches the
  generation ceiling (`entity-too-dense`, the signal to split the topic into
  subsections). Thresholds live in [limits](./project-settings.md#limits).

## Convergence

The build is done when:

- a full wave proposes zero mutations (a fixed point),
- and the checks pass or their findings are recorded as diagnostics.

A hard per-build turn budget backstops the loop. Work still open when the budget runs out
is parked in `status.yaml` and reported as an `incomplete-build` diagnostic. The next
build resumes parked items first. Unfinished work is never silent.

The fix-up pass also re-enqueues documents holding requirements whose `quote` no longer
locates, so a stale anchor left behind by a failed turn is retried on the next build
instead of lingering as a `stale-provenance` warning.

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
