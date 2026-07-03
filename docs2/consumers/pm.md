# Project management

Project management maps the semantic graph onto a tracker (issues, epics, boards). It reads
the graph through the [context engine](../compiler/context.md) and the
[read tools](../compiler/tools.md#read-tools), never the raw source files.

## Graph to work items

- Each [entity](../compiler/model/entity.md) maps to a tracker work item.
- Each [requirement](../compiler/model/requirement.md) maps to an acceptance criterion on
  that item, quoted from the docs, or to a sub-task for large items.
- [Relationships](../compiler/model/relationship.md) map to structure: `composition` and
  `aggregation` become parent and child items, `dependency` becomes a blocking link.

## Stable ids are the traceability key

Work items are keyed by node id ([identifiers](../compiler/model.md#identifiers)). The same
id binds the spec (the graph node), the code unit ([code generation](./codegen.md)), the
tests ([test generation](./testgen.md)), and the ticket. Re-syncing is idempotent: an
existing key updates its item and creates no duplicates. A merged entity leaves a redirect
([mutations](../compiler/graph.md#mutations)); the sync follows it and folds the absorbed
item into the survivor's.

## Diagnostics as a review queue

Open [diagnostics](../compiler/model/diagnostic.md) with their triage state form a review
queue: contradictions, ambiguities, and coverage gaps, ordered by severity and
`confidence`. Diagnostics are graph nodes, so triage survives rebuilds and the queue does
not reset when the build reruns.

## Release diffs from the journal

The [journal](../compiler/graph.md#journal) records every committed changeset. The journal
entries between two builds are the release diff: which entities and requirements changed,
and the recorded `reasoning` for each change. That scopes release notes, review effort, and
regression testing to what actually moved.
