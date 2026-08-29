# The rejudge-pair goal

`rejudge-pair` judges one pair of requirements: one that was created or revised,
against one neighbor the reconciler picked because it overlaps it. The session owes
exactly one verdict: duplicate (delete the worse-sourced one, or file
`duplicate-requirement` for intentional cross-document redundancy), contradiction (file
`contradiction`, with a [prompt](../model/diagnostic.md#prompts) when the repair is
enumerable), or consistent (no action, stated in the justification). It also settles an
open pair diagnostic whose condition lapsed or whose other subject died. Focused
pairwise judgment is deliberate: a model answers "can these two both hold?" far more
reliably than "is this entity coherent?". The reach is lexical by design, and the
[`review-entity`](./review-entity.md) goal is the net for conflicts a pair cannot
reach.

- Class: compile. Mandatory. Readiness tier 2.
- Unit: one pair. Goal id `g:rejudge-pair:req:a~req:b`, the smaller id first, e.g.
  `g:rejudge-pair:req:orders-6~req:payment-9`.
- Skill: [`judgment`](../skills/judgment.md).

## Created when

Three sources derive the goal ([pairs](../reconciler.md#pairs)):

- `requirement-created` and `requirement-revised` [change records](../graph.md#change-records):
  a commit writes one when it creates a requirement or changes its `statement` or its
  quote in substance (normalized text, not punctuation). For each such record the
  reconciler computes the neighbor set deterministically: requirements sharing an
  entity with it, scored by overlapping content tokens (statement tokens minus stop
  words and the shared entities' own name tokens, reduced to crude stems), at least two
  shared tokens, best six by score. One goal per neighbor.
- Sticky pairs: an open `contradiction` or `duplicate-requirement` diagnostic ties two
  requirements. When either side is created or revised, the other is a neighbor
  whatever the token overlap says, so editing one side of a known pair always re-judges
  the other.
- `node-deleted` on a surviving subject of an open pair diagnostic: the sweep writes it
  when the other subject died. The goal shows the dead subject marked `(deleted)`; the
  session resolves the diagnostic when the finding died with the requirement, or
  refiles it against the surviving statements. A diagnostic left with no existing
  subject at all is settled by the store at commit (`settle-diagnostics`,
  [journal](../graph.md#journal)); no goal derives for it.

A changed requirement with no neighbor, no sticky partner, and no open diagnostic
naming it derives no pair goal. Two changed requirements that are each other's
neighbors are one goal: judging A against B is judging B against A. One
`requirement-*` record feeds up to six goals, so it clears when the last of them
resolves; until then its `detail.judged` lists the neighbors already judged, and a
re-derivation skips them.

E.g.:

```yaml
- id: c413-2
  generation: 413
  mutation: 2
  kind: requirement-revised
  subject: req:orders-6
  via: quote
  detail: {fields: [statement, quote], judged: []}
```

derives, for each neighbor:

```yaml
g:rejudge-pair:req:orders-6~req:payment-9:
  kind: rejudge-pair
  class: compile
  mandatory: true
  target: req:orders-6~req:payment-9
  unit: pair
  change: {revised: req:orders-6, fields: [statement, quote],
           shared: {entity: ent:order, tokens: [hold, expire]}}
  cause: {generation: 413, mutation: 2, via: entities}
  state: open
```

`change` is the goal's identity across re-derivations: a pair judged consistent in one
build derives again only when a side changes in substance again.

## Readiness

- Tier 2: ready when no tier 0 or 1 goal is open or parked
  ([readiness](../reconciler.md#readiness)). Both sides' sections are settled before
  the statements are judged; a statement still being extracted is not worth judging.
- Locality is the node neighborhood ([batching](../reconciler.md#batching)): a changed
  requirement's pair goals batch together, and with the `review-entity` goals of the
  entities they share when the budget allows, pairs first in the batch order so the
  entity review sees the pairs' verdicts. A changed requirement loaded once serves all
  its pairs.
- A statement the session repairs before judging (`update_requirement` on a drifted
  `statement`) writes a new `requirement-revised` record at commit and the pair derives
  again with the new cause ([bubbling](../reconciler.md#bubbling)). The second judgment
  stages nothing, and the cascade ends there: that is the fixed point.

## Gate

`mark_goal_done({goal, justification, evidence})` carries `evidence` naming the verdict
for the pair and, for the two acting verdicts, what carried it:

```yaml
evidence: {verdict: consistent}
evidence: {verdict: duplicate, carried_by: req:payment-9}         # the deleted side
evidence: {verdict: duplicate, carried_by: diag:duplicate-requirement-4}
evidence: {verdict: contradiction, carried_by: diag:contradiction-3}
```

The harness validates the claim over the store plus what the session has staged:

- `verdict` is one of `duplicate`, `contradiction`, `consistent`. A claim without one is
  rejected naming the gate.
- `duplicate`: a `delete_requirement` on one side is staged in this session, or an open
  `duplicate-requirement` diagnostic naming both ids is staged or recorded.
- `contradiction`: an open `contradiction` diagnostic naming both ids is staged or
  recorded.
- `consistent`: neither side is deleted in this session, and an open `contradiction` or
  `duplicate-requirement` diagnostic on the pair, if one exists, is resolved in this
  session. A consistent verdict on a sticky pair is a claim that the old finding
  lapsed, and the diagnostic must say so.
- A pair with a subject marked `(deleted)`: the diagnostic naming it is resolved in this
  session, refiled or not.
- The justification is present. Brevity is the contract's demand and the journal
  records it beside the goal ([journal](../graph.md#journal)).

`done` runs the same gate over every goal in the batch and the per-mutation gates on
what was staged ([validation gates](../graph.md#validation-gates)): `report_diagnostic`
subjects exist, a prompt's `old_text` locates in the section it names, a
`delete_requirement` carries a reason. Staging nothing is the common correct outcome:
every pair consistent, one `mark_goal_done` each, `done` with a one-line summary.

The gate verifies completeness, not correctness: it checks that a verdict with a
justification exists per pair; it cannot know a `consistent` verdict is true. Verdict
quality is the skill's demand and a benchmarking concern
([judgment](../concepts/judgment.md)). `mark_goal_failed({goal, reason})` is for a pair
the session cannot honestly decide: a quote that contradicts its own statement in a way
no `update_requirement` can repair without re-extraction, or two statements whose
documents changed under the session. A failed goal keeps its record and surfaces on
both requirements; it blocks convergence.

## Hints

Computed by the harness and rendered under the goal block:

- Why the pair exists: `revised req:orders-6 (statement, quote); shares ent:order and
  tokens hold, expire`, or `sticky: diag:contradiction-3`, or
  `req:payment-9 (deleted in g413: duplicate); diag:contradiction-3 names it`.
- Whether the two quote the same document: same document, delete the worse-sourced
  duplicate; different documents, keep both and file `duplicate-requirement`.
- The open diagnostics naming either side, with `(deleted)` on dead subjects.
- `load req:<id>` when a side is not in the loaded set in full.
- `skill judgment`, and the tool per verdict: duplicate, `delete_requirement` or
  `report_diagnostic` `duplicate-requirement`; contradiction, `report_diagnostic`
  `contradiction` with a prompt; consistent, `mark_goal_done` alone.

## What the model sees

The session prompt is [assembled](../sessions.md#the-prompt) like every session's: the
agent contract, the active skills, the project block, the goals block, the loaded set.
The goal block carries the contract paragraph from
[`prompts/rejudge-pair.md`](./prompts/rejudge-pair.md): ground the verdict in the
quotes as much as the statements, repairing a drifted statement first; decide exactly
one verdict; duplicate means the same obligation reworded, not the same topic, deleted
when both quote one document and filed as `duplicate-requirement` when they quote two;
contradiction means the two cannot both hold, filed with severity `error` when no
reading lets both hold and `warning` otherwise, with one `edit` option per side when
the repair is enumerable; consistent means both can hold and state different facts;
different behavior for one subject contradicts, it does not duplicate; resolve a lapsed
diagnostic and refile one naming a dead subject; when in doubt keep both and file. Then
the change in one line, the gate in one line, and the hints.

The `judgment` skill is active from the first round: the three verdicts, the review
asymmetry, prompts, findings, severity, verdict quality
([the review asymmetry](../concepts/judgment.md#the-review-asymmetry)).

The initially loaded set for the batch holds:

- Both requirements in full: `statement`, quote, section, entities, edges, transition,
  facets, `confidence` and `reasoning`; the changed side marked with the fields that
  changed.
- The shared entity as a stub (name, one definition line, stereotype, edge count).
- The open diagnostics naming either requirement, each with its rule, severity,
  message, and subjects, `(deleted)` where a subject is gone.
- The other pairs of the batch share what is loaded: a changed requirement loaded once
  serves its six pairs.

E.g.:

```
## Goals
- [g:rejudge-pair:req:orders-6~req:payment-9] mandatory
  [contract paragraph]
  Change: req:orders-6 revised in g413 (statement, quote); shares ent:order, tokens hold, expire.
  Gate: a verdict in evidence (duplicate, contradiction, consistent), carried by a mutation or diagnostic.
  Hints: same document: no; no open diagnostic on the pair; skill judgment

## Loaded (4.2k/24k chars)
- req:orders-6    full (revised: statement, quote)   docs/orders.md#/orders/holds
- req:payment-9   full   docs/payment.md#/payment/holds
- ent:order       stub (definition only)   [7 requirements loadable: h:ent:order:requirements]
skills: judgment (active); extraction, flow-views, structural-views, abstraction, conformance (load_skill)
```

`jazyk preview <goal>` renders the prompt before it is spent
([preview](../sessions.md#preview)).

## Tools

The `rejudge-pair` toolset ([toolsets](../tools.md#toolsets)):

- The [read tools](../tools.md#read-tools): `load`, `expand`, `unload`, `graph_status`,
  `search`, `read_section`, `get_entity`, `get_view`, `diagnostics`.
- The [goal tools](../tools.md#goal-tools): `mark_goal_done`, `mark_goal_failed`,
  `load_skill`, `done`.
- `update_requirement({id, statement?, ...})`: repair a statement that drifted from its
  quote before judging. A call that only changes the `statement` passes `id` and
  `statement`; `section` and `quote` re-anchor the provenance and are for a quote that
  itself must move.
- `delete_requirement({id, reason})`: the worse-sourced same-document duplicate.
- `report_diagnostic({rule, severity, subjects, message, reasoning, prompt?})`, rules
  `duplicate-requirement` and `contradiction`.
- `resolve_diagnostic({id, reason})`: a pair diagnostic whose condition lapsed, or one
  naming a dead subject.
- [`report_feedback`](../tools.md#feedback-tool).

No entity mutations and no coverage: this goal judges statements. The error asymmetry
governs every verdict: a wrong delete destroys information, a missed duplicate only
leaves a finding for the next build. A contradiction or duplicate found against a
statement the batch did not pair is filed with `report_diagnostic` all the same,
provided the evidence is in quotes the session has read; the diagnostic makes the pair
sticky, and the next change on either side re-judges it.
