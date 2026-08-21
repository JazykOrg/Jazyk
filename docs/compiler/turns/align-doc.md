# The align-doc turn

Goal: decide where the anchors that [alignment](../alignment.md) could not place with
certainty now belong in one document. For each proposal the model compares the previous
location and wording with the candidates and either places the anchor as it stands,
places it and marks it for re-evaluation, or leaves it homeless. The turn writes no
entities, no requirements, no coverage, and no diagnostics: it moves provenance and sets a
flag, and the `reconcile-doc` turn that follows does the rest.

## What the model sees

One prompt, assembled in this order:

1. The [system prompt](./prompts/align-doc.md), with the
   [feedback note](./prompts/feedback-note.md) inserted as its second paragraph.
2. The work pack ([template below](#the-work-pack)).
3. The [worker protocol](./prompts/worker-protocol.md) line, `{target}` replaced by
   the document path.

Delivery is the same as for [reconcile-doc](./reconcile-doc.md#what-the-model-sees): the
whole prompt as an ACP session prompt, or the same text as `instructions` plus `package`
in the `begin_compilation` reply over MCP.

## The work pack

Assembled deterministically by the harness (`align_pack`), budgeted by
`limits.context_budget`:

```text
# Work item: align anchors in {doc}

## Section changes (computed)
- moved: {old doc}#{ref} → {doc}#{ref} (similarity {pct})
- split: {old doc}#{ref} → {doc}#{ref}, {doc}#{ref}
- merged: {old doc}#{ref}, {old doc}#{ref} → {doc}#{ref}
- edited: {doc}#{ref} (similarity {pct})
- deleted: {old doc}#{ref}

## Proposals (decide every one)

### {req}: {ears}
was: {old doc}#{ref} "{quote}"
  {old excerpt}
candidates:
  1. {doc}#{ref} ({title}) similarity {pct}, quote locates: yes
     {new excerpt around the located text}
  2. {doc}#{ref} ({title}) similarity {pct}, quote locates: no, nearest: "{nearest}"
     {new excerpt around the nearest text}

### {ent} (entity {name}), mention
was: {old doc}#{ref} "{quote}"
  {old excerpt}
candidates:
  1. ...
```

How each block is selected:

- `Section changes`: every operation the deterministic pass computed for this document
  and for the old sections whose anchors it now holds, so the model knows whether it
  is looking at a rename, a move, a split, or a merge before it reads a single quote.
- `Proposals`: the document's pending block in `status.yaml`, one entry per anchor,
  candidates in descending similarity, capped at 3 per anchor. Excerpts are the quote
  (or `nearest` text) with up to 3 lines of context on each side, each given an equal
  share of the remaining budget; an over-budget candidate keeps a `read_section`
  pointer.

## Tools

The `align-doc` toolset (see [task toolsets](../tools.md#task-toolsets)): `context`,
`expand`, `search`, `read_section`, `get_entity`, `place_anchor`, `orphan_anchor`, `done`,
plus `report_feedback` ([feedback tool](../tools.md#feedback-tool), present in every
toolset). No entity or requirement mutation, no `set_coverage`, no `report_diagnostic`:
the turn places, it does not extract or judge.

- `place_anchor({id, section, quote?, reevaluate})`: moves the anchor to `section`. A
  `quote` must locate there and replaces the stored quote; without one the stored quote
  stays. An entity listed with several mentions is decided once; all of them move.
  `reevaluate: true` lists the anchor as a stale anchor on the target document's
  `reconcile-doc` item; a quote that does not locate has the same effect whatever the
  flag says.
- `orphan_anchor({id})`: no home. The anchor stays a stale anchor on its old document.

The rule of thumb the prompt states: place as-is when the same statement is made in a
place that still governs the same subject; re-evaluate when the wording, the scope, or
the surrounding section changed meaning; orphan when the statement is gone.

## Finish

`done` is rejected while a proposal in the work item is undecided (`undecided-proposal`),
naming each one. The changeset of `place_anchor` and `orphan_anchor` mutations commits
atomically, journaled under task `align-doc`, and the document's `alignment` block is
cleared, which unblocks its `reconcile-document` task.
