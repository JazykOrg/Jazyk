# Judgment

`reasoning` is the recorded why, stored next to what it explains. See
[shared fields](../model.md#shared-fields). It appears in two places, but it is one idea:

- On extracted facts (entities, requirements): why the fact is shaped this way, drawn from
  the documents' own explanation. E.g. "email must be unique because it is the login
  identifier."
- On [diagnostics](../model/diagnostic.md): why this severity was chosen at an ambiguity
  point. The compiler's judgment is LLM-backed, so recording it makes the call auditable.

The [journal](../graph.md#journal) keeps the reasoning given during each turn, so the
audit trail explains the graph, not just describes it.

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

## Calibration

Documentation is loose by design. The compiler calibrates against that, not against a
formal spec:

- The compiler shall flag only findings the document author can act on.
- It does not demand formal-spec completeness from prose. Missing persistence details,
  versioning schemes, or exhaustive case enumeration are not findings.
- A diagnostic's severity shall stay stable across builds unless the underlying facts
  materially change. Diagnostics are sticky nodes, reconciled rather than regenerated, so
  a rebuild over unchanged documents does not reshuffle severities. See
  [diagnostic](../model/diagnostic.md).
