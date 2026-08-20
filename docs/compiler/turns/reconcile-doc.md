# The reconcile-doc turn

Goal: bring the graph in line with one document's changed sections. The model reads
each dirty section, extracts requirements and the entities they need, updates what
drifted, and marks every dirty section covered or non-normative. It records the facts
as stated, even when sections disagree: judging is the review turns' job, and
`report_diagnostic` is not in this toolset.

## What the model sees

One prompt, assembled in this order:

1. The [system prompt](./prompts/reconcile-doc.md), with the
   [feedback note](./prompts/feedback-note.md) inserted as its second paragraph.
2. The work pack ([template below](#the-work-pack)).
3. The [worker protocol](./prompts/worker-protocol.md) line, `{target}` replaced by
   the document path.

Delivery depends on who runs the turn, but the text is the same
([one source](./../turns.md#task-types)):

- As an [ACP worker session](../../frontends/acp.md#worker-sessions): the whole
  prompt is the session prompt. The injected `jazyk mcp compile --only <doc>
  --packaged` serving answers `begin_compilation` with a short ack, because the
  contract already arrived as the prompt.
- Over plain [MCP](../../frontends/mcp.md#compilation-over-mcp): the
  `begin_compilation` reply carries the same system prompt as `instructions` and the
  same pack as `package`.

## The work pack

Assembled deterministically by the harness (`reconcile_pack`), budgeted by
`limits.context_budget`:

```text
# Work item: reconcile document {doc}
sections: {total} total, {n} with coverage

## Linked from (what other documents already say this one details)
- {doc}#{section} "{quote}" introduced {ent} ({name})
- {doc}#{section} "{quote}" states {req} ({ears})

primarySubject: {ent} ({name}). This document details that entity: ...
(or) candidateSubjects: {ent} ({name}), ...: each statement's own section decides ...

## Known entities (search before creating new ones)
- {ent} ({name}): {definition}
- (and {n} more; use search)

## Stale anchors (their source text changed or vanished; re-anchor, update, or delete)
- {req}: {ears} (in {doc}#{section}; was quoted: "{quote}")
- {ent} (entity {name}): a mention's section changed

## Dirty sections

### {doc}#{ref} ({title}) [coverage: {state}]
{section body, whole, or truncated with a read_section pointer}
Already extracted from this section (leave unchanged statements alone):
- {req}: {ears}
```

How each block is selected:

- `Linked from`: mentions and requirement quotes in other documents whose markdown
  links resolve to this document, capped at 12. When they name exactly one entity,
  `primarySubject` states it; several give `candidateSubjects`. This is how the turn
  knows what "the system" means without guessing. See
  [incoming links](./../turns.md#incoming-links).
- `Known entities`: entities mentioned in this document first, then the rest of the
  graph, capped at 40 lines with a `use search` pointer for the remainder.
- `Stale anchors`: the work item's list, each with its statement and the quote that
  no longer locates. The `done` gate rejects the turn while one is untouched.
- `Dirty sections`: every dirty section's body, each given an equal share of the
  remaining context budget; an over-budget body is truncated with a `read_section`
  pointer. Under each body, the requirements already sourced from that section, so
  an unchanged statement is a no-op and a coverage claim sees what the section
  already yielded.

## Tools

The `reconcile-doc` toolset (see [task toolsets](../tools.md#task-toolsets)):
`context`, `expand`, `search`, `read_section`, `upsert_entity`, `update_entity`,
`delete_entity`, `upsert_requirement`, `update_requirement`, `delete_requirement`,
`set_coverage`, `done`, plus `report_feedback`
([feedback tool](../tools.md#feedback-tool), present in every toolset).
`report_diagnostic` is deliberately absent: extraction records, review judges.

## Finish

`done` runs the batch gates ([validation gates](../graph.md#validation-gates)): every
staged quote locates, every dirty section carries a coverage mark, every stale anchor
is addressed, and a `covered` claim has a requirement behind it. A rejection names
the violated rule; the turn repairs exactly that and calls `done` again. The
changeset commits atomically or not at all.
