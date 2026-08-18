# Compilation

A build brings the [graph](./model.md) in line with the documents. The
[reconciler](./reconciler.md) computes what is stale and orders the work; this file
describes what a build does with it, from dirty set to verdict. The
[control plane](./control-plane.md) decides whether anyone may act on the work at all.

```
parse all docs → diff section trees → dirty set
  → ingest wave (reconcile-doc turns, root first, then levels in parallel)
  → fix-up (unprocessed sections and unlocated quotes re-enqueue once)
  → pair review wave (review-requirement turns for changed statements)
  → review wave (review-entity turns, grouped)
  → checks (deterministic lint, coverage, reachability, document quality)
  → fixed point reached, or budget exhausted with work parked
```

The first build and every rebuild run the same lifecycle. The first build starts
from an empty graph, so everything is dirty. A rebuild with no changes has an empty
dirty set and makes zero LLM calls.

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

## Convergence

The build is done when:

- a full wave proposes zero mutations (a fixed point),
- and the checks pass or their findings are recorded as diagnostics.

A hard per-build turn budget backstops the loop. Work still open when the budget runs out
is parked in `status.yaml` and reported as an `incomplete-build` diagnostic. The next
build resumes parked items first. Unfinished work is never silent.

The verdict in `status.yaml` is `converged` only when nothing is parked, no review is
pending in the [task queue](./reconciler.md#the-task-queue), and no section with a body of its own is
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
