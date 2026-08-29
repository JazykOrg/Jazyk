# Project management

Project management maps the semantic graph onto a tracker (issues, epics, boards). It reads
the graph through the [loaded set](../compiler/context.md#the-loaded-set) and the
[read tools](../compiler/tools.md#read-tools), never the raw source files.

## Graph to tracker items

- Each [entity](../compiler/model/entity.md) maps to a tracker item. The `parent`
  tree maps to the item hierarchy: a containing entity is the epic, its children the
  items under it ([containment](../compiler/model/entity.md#containment)). The
  `stereotype` is the item's label.
- Each [requirement](../compiler/model/requirement.md) maps to an acceptance criterion on
  that item, its `statement` with the verbatim quote from the docs, or to a sub-item for
  large items. A derived or decreed requirement carries its ratification proposal instead
  of a quote until the documents state it.
- [Relationships](../compiler/model/relationship.md) map to structure: `composition` and
  `aggregation` become parent and child items where `parent` does not already say so,
  `dependency` and `realization` become blocking links.
- A flow [view](../compiler/model/view.md) (use case, sequence) maps to a story grouping
  its member requirements in flow order.
- A ledger row maps to the item's verification state
  ([status is derived](./gen.md#status-is-derived-never-stored)): `verified`,
  `unimplemented`, `failing`, or stale.

## Stable ids are the traceability key

Tracker items are keyed by node id ([identifiers](../compiler/model.md#identifiers)). The
same id binds the spec (the graph node), the implementing files and the tests
([generation](./gen.md)), and the ticket. Re-syncing is idempotent: an existing key
updates its item and creates no duplicates. A merged entity leaves a redirect
([mutations](../compiler/graph.md#mutations)); the sync follows it and folds the absorbed
item into the survivor's.

## Diagnostics as a review queue

Open [diagnostics](../compiler/model/diagnostic.md) with their triage state form a review
queue: contradictions, ambiguities, invented choices, and coverage gaps, ordered by
severity and `confidence`. Diagnostics are graph nodes, so triage survives rebuilds and
the queue does not reset when the build reruns. Goals blocked on a human (`ratify`,
`answer`) sit in the same queue: each names the fact or diagnostic waiting on a decision
([the ratify goal](../compiler/goals/ratify.md),
[the answer goal](../compiler/goals/answer.md)). The GUI board renders the same goals as
cards ([board](../frontends/gui.md#board)).

## Release diffs from the journal

The [journal](../compiler/graph.md#journal) records every committed changeset, one entry
per generation: the mutations, the goals the changeset resolved (`resolved_goals`, each
with its one-line justification), and the goals it opened (`opened_goals`, each with its
cause). The journal entries between two builds are the release diff:

- which entities, requirements, and views changed, with the recorded `reasoning` for each
  change,
- which goals were resolved, with their justifications, so the diff explains why each
  change happened and not only that it happened,
- which goals were opened and still stand (open, blocked, parked, or failed), so the
  diff names the work a release leaves behind,
- which human edits started each cascade (`edit`, `dual-write`, `decree`, `ratify`, and
  `answer` entries), so the diff separates what people decided from what followed.

That scopes release notes, review effort, and regression testing to what actually
moved. `jazyk ripple <generation>` renders the same entries as the causal tree of one
build, cost beside it ([CLI](../frontends/cli.md#jazyk-ripple)).
