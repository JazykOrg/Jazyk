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

The `change` carries two id lists, `none` (no criterion yet) and `revised` (a derived
criterion gone stale); the `cause` is the newest record among them. Each goal is a batch wall of its own
([batching](../reconciler.md#batching)), and the changeset's mutation budget grows by
three per listed requirement ([budgets](../sessions.md#budgets)): a batch that packed
twelve entities' goals against one budget once failed three of them unstaged. A project without a deliverable derives none:
stated criteria still enter through extraction, and the `criterion-missing` check
reports the gaps as information ([checks](../compilation.md#checks)).

## Readiness

Tier 3, with the ledger work: no tier 0, 1, or 2 goal open or parked, so pairs and
reviews have settled the statements the criteria describe. `bind` on a requirement waits
for at least one criterion on it ([tiers](../reconciler.md#tiers)); `derive-criteria`
runs first by construction.

## Gate

`mark_goal_done({goal, justification, evidence})` carries one entry per requirement the
`change` listed: `{id, criteria: [crit ids], verdict: complete | stated-only | open-question}`,
under `requirements` (`bad-evidence` for another shape). A listed requirement without an
entry is refused (`criteria-unjudged`), naming it.

- `complete`: at least one criterion is staged or recorded on the requirement
  (`criterion-missing` otherwise), and no staged criterion has the same normalized text
  as the requirement's statement (`criterion-restates` otherwise).
- `stated-only`: the documents' own criteria cover the requirement and the session
  added none; at least one criterion exists (`criterion-missing` otherwise).
- `open-question`: the requirement's evidence depends on a value the documents do not
  give, or give twice, and an open diagnostic with a prompt names the requirement: an
  `invented-choice` staged here for a value nobody stated
  ([invented choices](../../consumers/gen.md#invented-choices)), or a standing
  `contradiction`, `decision`, or `ambiguity` whose prompt already carries the
  question (the 21 versus 30 day deadline is that case; filing a second question
  would be a twin; `unbacked-question` otherwise). The requirement keeps
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

The entity as a stub, each listed requirement in full with its criteria under it, and
the source section of each listed requirement in full, once per section: the values a
criterion needs live there, and a session that had to fetch them spent one to nine
`read_section` rounds before its first criterion (every criteria session of one f2
run did). A section past the loaded set's budget arrives as a handle. The contract paragraph is
`goals/prompts/derive-criteria.md`.

## Tools

The `derive-criteria` toolset ([toolsets](../tools.md#toolsets)):

- The [read tools](../tools.md#read-tools): `load`, `expand`, `unload`, `graph_status`,
  `search`, `read_section`, `get_entity`, `get_view`, `diagnostics`.
- The [goal tools](../tools.md#goal-tools): `mark_goal_done`, `mark_goal_failed`,
  `load_skill`, `done`, and [`report_feedback`](../tools.md#feedback-tool).
- `upsert_criterion({requirement, statement, kind, measure?, given?, when?, then?,
  entities?, section?, quote?, provenance?})`: a stated criterion carries `section` and
  `quote`; a derived one carries `provenance: {derived: {from: [req id], reasoning}}` and
  the proposal follows at commit.
- `update_criterion({id, statement?, kind?, measure?, given?, when?, then?})`.
- `delete_criterion({id, reason})`.
- `report_diagnostic` (`invented-choice`, `ambiguity`).
