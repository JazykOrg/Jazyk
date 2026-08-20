# The review-entity turn

Goal: judge one entity whose facts changed. The model checks that the requirement
set is coherent, refreshes the `definition` when it drifted, merges lookalike
entities that are one concept, adds missing entity references, deletes
same-document duplicate requirements, files [diagnostics](../model/diagnostic.md)
(contradiction, duplicate-entity, ambiguity, missing-link, lint), and settles open
diagnostics that no longer hold. It sees the entity's whole statement set, which
makes it the net for conflicts the [pairwise review](./review-requirement.md)
cannot pair lexically.

## What the model sees

One prompt, assembled in this order:

1. The [system prompt](./prompts/review-entity.md), with the
   [feedback note](./prompts/feedback-note.md) inserted as its second paragraph.
2. The work pack ([template below](#the-work-pack)).
3. The [worker protocol](./prompts/worker-protocol.md) line, `{target}` replaced by
   the entity id.

Delivered like every compilation turn; see
[the reconcile-doc turn](./reconcile-doc.md#what-the-model-sees) for the shape.

## The work pack

The pack opens with the standard [entity context pack](../context.md) (definition,
aliases, mentions with one parent chain, all requirements, one hop of related
entities and their statements; over-budget lists become expansion handles). Then the
review-specific blocks:

```text
# Work item: review entity {ent}

## Entity {ent} ({name})
definition: {definition}
aliases: {aliases}

### Mentions
- {doc}#{section} "{quote}"

### Requirements
- {req}: {ears}

## Name-similar candidates (a shared word proves nothing; merge only when they are one concept)
- {ent} ({name}): {definition}

## Related but separate candidates (a field, part, or child concept; merge only with explicit evidence they are one concept)
- {ent} ({name}): {definition}

## Statements naming this entity without referencing it (add the reference if the statement is about it)
- {req}: {ears}
These candidates are word matches, not judgments: ...

## Open diagnostics on this entity's statements (resolve any that no longer hold)
- {diag}: ...

## Project lint rules
Report a violation with report_diagnostic, rule `lint`, and the severity listed.
- (warning) {rule text}
- (error) {rule text}
```

How each block is selected:

- Candidates: token-overlap hits on the entity's name, partitioned so a name that
  extends this one (or vice versa) lands under "related but separate": a child
  concept must never read as a merge suggestion.
- Unreferenced statements: requirements whose prose names this entity or an alias
  (word-bounded, code spans stripped) without referencing it, minus matches that
  belong to a referenced compound name, capped at six.
- Open diagnostics: those naming this entity's requirements; ones naming the entity
  itself already ride in the context pack.
- Lint rules: the project's [`[docs.linting]`](../project-settings.md#linting)
  rules, restated with their severities.

## Tools

The `review-entity` toolset (see [task toolsets](../tools.md#task-toolsets)):
`context`, `expand`, `search`, `get_entity`, `diagnostics`, `update_entity`,
`merge_entities`, `update_requirement`, `delete_requirement`,
`report_diagnostic`, `update_diagnostic`, `resolve_diagnostic`, `done`, plus
`report_feedback`. No creation tools: review judges what extraction recorded.

## Finish

`done` with a one-line summary. Staging nothing is a correct outcome when
everything is coherent. The error asymmetry is stated in the prompt: a wrong merge
or delete destroys information, a missed duplicate only leaves a finding for the
next build; when in doubt, keep both and file a diagnostic.
