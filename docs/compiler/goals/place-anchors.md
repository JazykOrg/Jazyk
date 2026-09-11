# The place-anchors goal

`place-anchors` decides where the anchors that [alignment](../alignment.md#anchor-relocation)
could not place with certainty belong in one document. An anchor is a requirement's
`source` quote or an entity mention. The deterministic pass applies exact moves by
itself and leaves the rest as proposals
([what applies and what is proposed](../alignment.md#what-applies-and-what-is-proposed));
this goal spends judgment on exactly those. For each proposal the session compares the
anchor's previous location and wording with the candidate sections and either places
the anchor as it stands, places it and marks it for re-evaluation, or leaves it
homeless. The session writes no entities, no requirements, no coverage, and no
diagnostics: it moves provenance and sets a flag, and the
[`reconcile-section`](./reconcile-section.md) goals that follow do the rest.

- Class: compile. Mandatory. Readiness tier 0.
- Unit: one document. Goal id `g:place-anchors:<doc path>`, e.g.
  `g:place-anchors:docs/orders.md`.
- Skill: none. The contract paragraph carries the whole rule.

## Created when

The goal derives from an `alignment-pending` [change record](../graph.md#change-records)
on the document. The [dirty set](../reconciler.md#dirty-set) computation writes it in
the `align` journal entry when the deterministic pass leaves at least one proposal for
the document: an anchor whose section was edited, split, merged, or deleted, with
candidates ranked by similarity, or with no candidate at all (`homeless`). The proposals
themselves live in `status.yaml` under `alignment`, one block per document; the
record's `detail` counts them, and the goal's `change` is that block.

E.g.:

```yaml
- id: c412-0
  generation: 412
  mutation: 0
  kind: alignment-pending
  subject: docs/orders.md
  via: alignment
  detail: {proposals: 3, ops: {edited: 1, split: 1, deleted: 1}}
```

`cause` is the `align` generation that persisted the proposals, the record's own
generation, with `mutation: 0` because the cause is store-level
([goal derivation](../reconciler.md#goal-derivation)). The document edit behind it is one
of the [edit paths](../compilation.md#edit-paths). One goal per document, whatever the
proposal count: the proposals of one document are decided together because the section
changes block that explains them is one computation. The record clears when the goal
resolves, which empties the document's `alignment` block. A document that gains
proposals again on a later edit derives the goal again with a new cause.

## Readiness

- Tier 0: always ready ([readiness](../reconciler.md#readiness)). Alignment runs before
  ingest because a `reconcile-section` goal on the document is not ready while the
  document carries an `alignment-pending` record: extraction over a section whose
  anchors are undecided would re-extract statements that exist under another section
  and mint duplicates.
- Locality is the document ([batching](../reconciler.md#batching)). A batch is one
  document's goal. Several documents' goals share a session only when their proposals
  are few and their loaded sets fit the context budget together.
- A parked goal of this kind resumes first next build. An undecided document blocks its
  own sections and nothing else.

## Gate

Every proposal in the document's `alignment` block is decided, by a `place_anchor` or
`orphan_anchor` staged in the session or already committed:

- `mark_goal_done({goal, justification})` is validated against the block plus what the
  session has staged. While one proposal is undecided the claim is rejected with
  `undecided-proposal`, naming each one.
- `done` runs the same gate over every goal in the batch
  ([validation gates](../graph.md#validation-gates)), on top of the per-mutation gates:
  a `place_anchor` quote must locate whitespace-insensitively in the target section,
  and an entity with several proposed mentions is decided by one call.
- An implicit `done` (the session ended with mutations staged and no `done`) commits
  the decided proposals, and the goal stays open on the rest: a placement is never
  discarded over a missing call, and an undecided proposal is never decided by the
  harness ([commit](../sessions.md#commit)).
- `mark_goal_failed({goal, reason})` is for a document whose candidates make no sense
  together (the text was replaced wholesale). A failed goal keeps its record and
  surfaces on the document; the anchors stay where they were, their quotes do not
  locate, and the `stale-provenance` [check](../compilation.md#checks) reports them
  until a human edits the document or a later session decides.

At commit the document's `alignment` block is cleared and the record with it. A
`reevaluate: true` placement, and a placement whose stored quote does not locate in its
new section, write an `anchor-stale` record on the target section, so that section's
`reconcile-section` goal lists the anchor as stale
([created when](./reconcile-section.md#created-when)). An orphaned anchor writes
`anchor-stale` on the nearest surviving ancestor of its old section, the document's
root section at worst, so the fact is re-recorded or deleted by a session that reads
the document.

## Hints

Computed by the harness and rendered under the goal block:

- The proposal count and the section changes that explain them: `moved`, `split`,
  `merged`, `edited`, `deleted`, each with its similarity
  ([phases](../alignment.md#phases)).
- Per proposal: the anchor's kind (requirement or mention), the candidate count, and
  whether the stored quote locates in the best candidate. `quote locates: yes` means
  placing as-is is the likely outcome; `no` means the wording changed and
  `reevaluate: true` or a fresh quote is likely.
- `load docs/<doc>#/<ref>` for a candidate section whose body is not in the loaded
  set.
- The tools that resolve the kind: `place_anchor` with `reevaluate`, `orphan_anchor`.

## What the model sees

The session prompt is [assembled](../sessions.md#the-prompt) like every session's: the
agent contract, the project block, the goals block, the loaded set. The goal block
carries the contract paragraph from
[`prompts/place-anchors.md`](./prompts/place-anchors.md): read the section changes
first; decide exactly one outcome per proposal; place as-is when the same statement is
made in a place that still governs the same subject; re-evaluate when the wording, the
scope, or the surrounding section changed what it means; orphan when no candidate makes
the statement any more; prefer placing over orphaning, and re-evaluating over placing
as-is when in doubt about meaning; a candidate that differs only in spelling,
punctuation, formatting, or list position means the same thing; write nothing else.
Then the change in one line, the gate in one line, and the hints. No skill is active
unless the batch holds another kind.

The initially loaded set for the document holds:

- The section changes block: every operation the deterministic pass computed for the
  document and for the old sections whose anchors it holds, so the model knows whether
  it is looking at a rename, a move, a split, or a merge before it reads a quote.
- The proposals, one item per anchor: the requirement's `statement` or the entity's
  name, the previous location and the stored quote with its old excerpt, and the
  candidates in descending similarity, capped at three per anchor, each with its
  section reference, title, similarity, whether the quote locates, the `nearest` text
  when it does not, and an excerpt of up to three lines of context on each side. An
  over-budget excerpt truncates with a `read_section` pointer
  ([policy](../context.md#policy)).
- The candidate sections as stubs, loadable by handle.

E.g.:

```
## Goals
- [g:place-anchors:docs/orders.md] mandatory
  [contract paragraph]
  Change: 3 proposals (1 edited, 1 split, 1 deleted), aligned in g412.
  Gate: every proposal decided.
  Hints: req:orders-6 quote locates in candidate 1; ent:hold mention: 2 candidates;
  load docs/orders.md#/orders/expiry

## Loaded (6.1k/24k chars)
- docs/orders.md   section changes: 1 edited, 1 split, 1 deleted
- proposals docs/orders.md   3 anchors, 5 candidates
- docs/orders.md#/orders/expiry   stub   [loadable: h:docs/orders.md#/orders/expiry:body]
Skills: none active; extraction, judgment, flow-views, structural-views, abstraction, conformance (load_skill)
```

`jazyk preview <goal>` renders the prompt before it is spent
([preview](../sessions.md#preview)).

## Tools

The `place-anchors` toolset ([toolsets](../tools.md#toolsets)):

- The [read tools](../tools.md#read-tools): `load`, `expand`, `unload`, `graph_status`,
  `search`, `read_section`, `get_entity`, `get_view`, `diagnostics`.
- The [goal tools](../tools.md#goal-tools): `mark_goal_done`, `mark_goal_failed`,
  `load_skill`, `done`.
- `place_anchor({id, section, quote?, reevaluate})`: moves the anchor to `section`. A
  `quote` must locate there and becomes the stored quote; without one the stored quote
  stays. An entity listed with several mentions is decided once; all of them move.
  `reevaluate: true` lists the anchor as stale on the target section's
  `reconcile-section` goal; a stored quote that does not locate in the new section has
  the same effect whatever the flag says.
- `orphan_anchor({id})`: no home. The anchor stays where it was, listed as stale for the
  document, and the `reconcile-section` goal that sees it re-records or deletes the
  fact.
- [`report_feedback`](../tools.md#feedback-tool).

No entity or requirement mutation, no `set_coverage`, no `report_diagnostic`: the
session places, it does not extract or judge. The rule of thumb the contract states:
place as-is when the same statement is made in a place that still governs the same
subject; re-evaluate when the wording, the scope, or the surrounding section changed
the meaning; orphan when the statement is gone. An orphaned anchor loses its id and its
history once the following session deletes it, so placing wins whenever a candidate
holds the statement, and re-evaluating costs one re-check, never information.
