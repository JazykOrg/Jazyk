# The review-requirement turn

Goal: judge ONE changed requirement against each of its neighbor statements. The
neighbors were picked deterministically by the reconciler
([waves](../compilation.md#waves)); the turn owes one verdict per neighbor:
duplicate (delete the worse-sourced one, or file `duplicate-requirement` for
intentional cross-document redundancy), contradiction (file `contradiction` with a
[prompt](../model/diagnostic.md#prompts) when the repair is enumerable), or
consistent (no action, stated only in the `done` summary). It also settles open
pair diagnostics whose condition no longer holds.

## What the model sees

One prompt, assembled in this order:

1. The [system prompt](./prompts/review-requirement.md), with the
   [feedback note](./prompts/feedback-note.md) inserted as its second paragraph.
2. The work pack ([template below](#the-work-pack)).
3. The [worker protocol](./prompts/worker-protocol.md) line, `{target}` replaced by
   the requirement id.

Delivered like every compilation turn: as the ACP worker session prompt, or as the
`begin_compilation` reply (`instructions` plus `package`) over plain MCP. See
[the reconcile-doc turn](./reconcile-doc.md#what-the-model-sees) for the shape.

## The work pack

```text
# Work item: review changed requirement {req} against its neighbors

## The changed requirement
- {req}
  ears: {ears}
  quote: "{quote}"
  section: {doc}#{section}

## Neighbors (one verdict each: duplicate, contradiction, or consistent)
- {req}
  ears: {ears}
  quote: "{quote}"
  section: {doc}#{section}

## Open diagnostics naming this requirement (resolve any that no longer hold)
- {diag}: {rule} {severity} on {subjects}: {message}
```

How each block is selected:

- `Neighbors`: recomputed with the same deterministic function that scheduled the
  turn: requirements sharing an entity, scored by overlapping content tokens
  (statement tokens minus stop words and entity names, crude-stemmed), at least two
  shared tokens, best six. Open `contradiction` and `duplicate-requirement`
  diagnostics add their partners as sticky neighbors. See
  [waves](../compilation.md#waves). The reach is lexical by design; the
  [entity review](./review-entity.md) is the net for conflicts that share no
  vocabulary.
- `Open diagnostics`: every open diagnostic naming this requirement as a subject. A
  subject deleted from the graph is marked `(deleted)`: such a diagnostic cannot
  stand as filed, and the turn resolves or refiles it.

## Tools

The `review-requirement` toolset (see [task toolsets](../tools.md#task-toolsets)):
`context`, `expand`, `search`, `get_entity`, `read_section`, `diagnostics`,
`update_requirement`, `delete_requirement`, `report_diagnostic`,
`update_diagnostic`, `resolve_diagnostic`, `done`, plus `report_feedback`. No
entity mutations and no coverage: this turn judges statements.

## Finish

`done` carries a one-line summary naming the verdict per neighbor. Staging nothing
is the common correct outcome (every pair consistent). The same batch gates apply
as everywhere: staged mutations validate or the turn repairs what the rejection
names.
