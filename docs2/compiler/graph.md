# Graph store

The graph store is the persistent home of the [semantic graph](./model.md). It owns
identifiers, enforces invariants, and records every change. The store is deterministic
code. No LLM runs inside it.

## Storage layout

The store lives in the project's out directory (default `jazyk-out/`). All files are YAML,
sorted by key, so builds diff cleanly in git.

```
jazyk-out/
  graph/
    entities.yaml        # map: id -> entity
    requirements.yaml    # map: id -> requirement
    relationships.yaml   # map: id -> relationship (derived, regenerated on commit)
    diagnostics.yaml     # map: id -> diagnostic
    redirects.yaml       # map: absorbed id -> surviving id
  docs/
    <mirrored doc path>.yaml   # per document: content hash, section tree, coverage
  journal/
    <build>-<seq>.yaml   # one file per committed changeset
  status.yaml            # generation counter, parked work, budgets spent, verdict
  .lock                  # single-writer lock
```

Each document file under `docs/` holds:

- `contentHash`: the hash of the source file at last reconcile.
- `sections`: map of internal reference → `title`, `kind`, `order`, `parent`, `raw`,
  `hash`, `lines`.
- `coverage`: map of internal reference → `state`, `note`, `claimedBy`. See
  [coverage](./reconciler.md#coverage).

## Mutations

A mutation is one operation on the graph. The full set:

- `upsert_entity`, `update_entity`, `delete_entity`, `merge_entities`
- `upsert_requirement`, `update_requirement`, `delete_requirement`
- `report_diagnostic`, `resolve_diagnostic`
- `set_coverage`

Mutations are exposed to the model as [write tools](./tools.md#write-tools). Their semantics:

- Upserts key on a natural key, not an id. For entities the natural key is `name` plus
  `scope`. For requirements it is the source section plus the punctuation-insensitive
  statement text, so a punctuation or spacing edit to a sentence matches its existing
  requirement and refreshes the `ears` and `quote` in place. An upsert that matches an
  existing node updates it instead of creating a duplicate. This makes retries harmless.
- The store mints ids. A mutation never supplies a new id, only references existing ones.
- Deletes require a `reason`, which is recorded in the journal.
- `merge_entities` keeps one entity, absorbs the other, rewires all requirement references,
  unions aliases and mentions, and writes a redirect from the absorbed id to the survivor.
  Downstream consumers holding the old id follow the redirect.

## Changesets

Mutations are not applied one by one. A [turn](./turns.md) stages mutations into a
changeset. The changeset commits atomically at the end of the turn:

- every staged mutation is applied,
- [derived data](#derived-data) is recomputed,
- the journal entry is written,
- the generation counter in `status.yaml` is bumped.

If the turn is aborted, the changeset is dropped and the graph is untouched.

Commits serialize on `.lock`. At commit time the store reconciles staged creates against
nodes committed concurrently by other turns: a staged create whose natural key now matches
an existing node becomes an update on that node, with mentions unioned. This makes parallel
ingest safe for same-name concepts. See [scheduling](./reconciler.md#scheduling).

## Validation gates

The store validates every mutation when it is staged and rejects invalid ones. A rejection
names the violated rule and how to repair the call, because the caller is a model that will
retry. E.g.:

```
unknown id `ent:cart`; nearest existing: `ent:shopping-cart`; use it, or create the entity first
```

The gates:

- Every referenced id must exist in the graph or earlier in the same changeset.
- Every `quote` must appear verbatim in its named section.
- An entity name that looks like syntax rather than a concept (a file path, a CLI flag,
  a markdown term) is rejected unless the call carries an explaining `note`.
- A requirement must reference at least one entity. Its `edges` may only tie entities the
  requirement itself references.
- A requirement statement must pass a lenient EARS shape check. See
  [EARS](./concepts/ears.md).
- `delete_entity` is rejected while any requirement still references the entity. The error
  lists the requirements.
- Diagnostic `subjects` must exist.
- `set_coverage` with state `non-normative` requires a `note`. A placeholder note
  (`<nil>`, `none`, `n/a`) counts as absent. Restaging coverage for a section replaces
  the earlier staged mark: one coverage mark per section per changeset.
- `upsert_entity` with a name variant of an existing entity (token containment, same
  scope) is rejected toward reuse plus an alias, unless a `note` says how they differ.
  See [entity](./model/entity.md#what-is-an-entity).

Batch-level gates run once more when the turn calls `done`: all quotes still locate,
coverage claims only touch the turn's target sections, every stale anchor in the work
item is addressed (its quote locates again, or a staged mutation re-records it under
its natural key, revises it, or deletes it), and a `covered` claim is honest.
A section may be claimed `covered` only when at least one requirement is sourced from
it. A section with nothing to extract is `non-normative` with a note, never silently
`covered`. This stops a turn from dropping a rejected requirement and claiming the
section anyway, and from skimming past declarative prose without extracting
([declarative prose states obligations](./concepts/ears.md#declarative-prose-states-obligations)).

## Derived data

- Relationships are a materialized view over requirements. On commit the store groups
  requirement `edges` by entity pair, unions the contributing requirements, and keeps the
  strongest implied type. See [relationship](./model/relationship.md). There is no write
  tool for relationships, so an edge cannot exist without a requirement behind it.
- A committed requirement adds its source as a mention on every entity it references
  (deduplicated). An entity reused by reference accumulates cross-document presence
  without an explicit `upsert_entity` call.
- The name index (name and alias → entity id) is rebuilt on load and after each commit.
  The [search tool](./tools.md#read-tools) queries it.

## Journal

Every committed changeset appends one journal file: the work item, the mutations applied,
rounds used, tokens spent, and the model's `reasoning` where given. The journal is the
audit trail of the build. It answers why the graph looks the way it does.

## Garbage collection

Cleanup is deterministic and never delegated to the model:

- A requirement whose source section disappeared and was not re-anchored during reconcile
  is deleted by the store, journaled.
- Entity mentions pointing at removed sections are pruned.
- An entity with zero mentions and zero requirements is deleted, with a tombstone redirect.

## Concurrency

- Writers take `.lock`. One changeset commits at a time.
- Readers (e.g. the [MCP server](../frontends/mcp.md)) do not lock. They read the generation
  counter, load shards, and retry if the counter moved mid-read.
