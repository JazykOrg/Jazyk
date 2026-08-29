# Judgment

`reasoning` is the recorded why, stored next to what it explains. See
[shared fields](../model.md#shared-fields). It appears in several places, but it is one
idea:

- On extracted facts (entities, requirements, views, attributes): why the fact is shaped
  this way, drawn from the documents' own explanation. E.g. "email must be unique because
  it is the login identifier."
- On [facets](../model/requirement.md#facets): why a statement is behavior, a
  constraint, a failure mode, or a quality.
- On derived provenance (`{from, reasoning}`): why the synthesized fact follows from its
  upstream nodes. See [provenance](../model.md#provenance).
- On [diagnostics](../model/diagnostic.md): why this severity was chosen at an ambiguity
  point. The compiler's judgment is LLM-backed, so recording it makes the call auditable.
- On goal resolutions: `mark_goal_done` carries a justification of one or two sentences,
  and `mark_goal_failed` a reason. See
  [resolving, failing, parking](../sessions.md#resolving-failing-parking).

The [journal](../graph.md#journal) keeps the reasoning given during each session and the
justification of every resolved goal, so the audit trail explains the graph, not just
describes it. `jazyk ripple` shows each justification beside its step. See
[`jazyk ripple`](../../frontends/cli.md#jazyk-ripple).

## Disposition of an ambiguity

The outcome of an ambiguity is graded by how much ambiguity remains after reading the
documents. The disposition and its reasoning are recorded:

| Ambiguity             | Disposition         | Recorded as                                                                     |
|-----------------------|---------------------|---------------------------------------------------------------------------------|
| none or trivial       | silent              | nothing                                                                         |
| small but real        | `none` (considered) | a diagnostic with severity `none` plus `reasoning`, hidden in the IDE by default |
| moderate              | `warning`           | a diagnostic plus `reasoning`                                                   |
| high or contradictory | `error`             | a diagnostic plus `reasoning`                                                   |

The severity `none` record is threshold-gated to avoid noise. It is kept when the
ambiguity is worth revisiting. It also gives continuity: if a later build raises the same
case to a warning, the earlier reasoning carries forward on the same diagnostic node.

A decision the documents leave open is a diagnostic too: rule `decision`, with a `prompt`
carrying the question and the options, answered by a human through the
[`answer`](../goals/answer.md) goal. See [prompts](../model/diagnostic.md#prompts).
Generation grades what it had to invent the same way: each invented choice is an
`invented-choice` diagnostic, error, warning, or suppressible info by the scope of the
invention. See [generation](../../consumers/gen.md).

## The review asymmetry

A wrong delete destroys information; a missed duplicate leaves a finding. When in doubt,
a session keeps both nodes and files a diagnostic. The asymmetry governs every judgment
goal: merges, deletes, and contradiction verdicts in
[`rejudge-pair`](../goals/rejudge-pair.md), [`review-entity`](../goals/review-entity.md),
and [`dedupe-candidates`](../goals/dedupe-candidates.md). The `judgment` skill
([`skills/judgment.md`](../skills/judgment.md)) carries it into those sessions.

Judgment gates verify completeness, not correctness. The `rejudge-pair` gate checks that
a verdict with reasoning exists per neighbor; it cannot know that a `consistent` verdict
is true. Verdict quality is measured by the [benchmark](../../benchmark/benchmark.md),
not by a gate.

## Calibration

Documentation is loose by design. The compiler calibrates against that, not against a
formal spec:

- The compiler flags only findings the document author can act on.
- It does not demand formal-spec completeness from prose. Missing persistence details,
  versioning schemes, or exhaustive case enumeration are not findings.
- It does not demand a sentence syntax from prose either. A declarative statement about
  the subject is extracted as a requirement, worded as a free-form statement. See
  [declarative prose states obligations](./statements.md#declarative-prose-states-obligations)
  and [wording](./statements.md#wording).
- A diagnostic's severity stays stable across builds unless the underlying facts
  materially change. Diagnostics are sticky nodes, reconciled rather than regenerated, so
  a rebuild over unchanged documents does not reshuffle severities. See
  [diagnostic](../model/diagnostic.md).
