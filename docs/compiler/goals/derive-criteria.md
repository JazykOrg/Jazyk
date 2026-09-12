# derive-criteria

The goal writes the [criteria](../model/criterion.md) of one entity's requirements: the
observable evidence each requirement is met, which [bind](../../consumers/bind.md) turns
into tests. Extraction already recorded the criteria the documents state (test cases,
examples, thresholds); this goal reads the requirements that still have none, or whose
statement changed since their criteria were written, and completes them.

## Created when

One goal per entity in the generation scope, `g:derive-criteria:<entity id>`, derived
when the project configures a `[gen]` deliverable
([project settings](../project-settings.md)) and the entity has a requirement that:

- carries no criterion, or
- carries a `requirement-revised` record newer than its criteria (its derived criteria
  were marked `criterion-revised` by the sweep, [change records](../graph.md#change-records)).

The `change` lists the requirements by id with the reason (`none` or `revised`); the
`cause` is the newest record among them. A project without a deliverable derives none:
stated criteria still enter through extraction, and the `criterion-missing` check
reports the gaps as information ([checks](../compilation.md#checks)).

## Readiness

Tier 3, with the ledger work: no tier 0, 1, or 2 goal open or parked, so pairs and
reviews have settled the statements the criteria describe. `bind` on a requirement waits
for at least one criterion on it ([tiers](../reconciler.md#tiers)); `derive-criteria`
runs first by construction.

## Gate

`mark_goal_done({goal, justification, evidence})` carries one entry per requirement the
`change` listed: `{id, criteria: [crit ids], verdict: complete | stated-only | open-question}`.

- `complete`: at least one criterion is staged or recorded on the requirement, and no
  staged criterion has the same normalized text as the requirement's statement
  (`criterion-restates` otherwise).
- `stated-only`: the documents state criteria and the session added none; at least one
  criterion exists.
- `open-question`: the requirement's evidence depends on a value the documents do not
  give, and an `invented-choice` prompt is staged naming the requirement
  ([invented choices](../../consumers/gen.md#invented-choices)). The requirement keeps
  no criterion until the answer; `bind` stays blocked on it.
- Every derived criterion staged carries `from` naming the requirement, `reasoning`, and
  a ratification proposal sentence toward the requirement's section
  ([ratify](./ratify.md)).

## Hints

- `requirements: <n> (<m> without criteria, <k> revised)`.
- For each listed requirement: id, statement, facets, transition, and the criteria it
  already has (id, kind, one line).
- `examples nearby: <section refs>`: sections of the requirement's document titled
  Examples, Test cases, or holding a table or a code block with values, which extraction
  may have left for this goal.
- `skill criteria`.

## What the model sees

The entity as a stub, each listed requirement in full with its criteria under it, the
sections the hints name loadable by reference. The contract paragraph is
`goals/prompts/derive-criteria.md`.

## Tools

- `upsert_criterion({requirement, statement, kind, measure?, given?, when?, then?,
  entities?, section?, quote?, provenance?})`: a stated criterion carries `section` and
  `quote`; a derived one carries `provenance: {derived: {from: [req id], reasoning}}` and
  the proposal follows at commit.
- `update_criterion({id, statement?, kind?, measure?, given?, when?, then?})`.
- `delete_criterion({id, reason})`.
- `report_diagnostic` (`invented-choice`, `ambiguity`).
- `read_section`, `load`, `search`, and the goal tools.
