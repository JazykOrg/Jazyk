# Diagnostic

A diagnostic is a first-class node recording a judgment: a contradiction, an ambiguity,
an uncovered section. Because diagnostics are nodes, they keep their id across builds and
get edited, not regenerated. Human triage survives recompilation by construction.

Diagnostics enter the graph through the `report_diagnostic` and `resolve_diagnostic`
[write tools](../tools.md#write-tools), used by review turns and by the deterministic
checks alike.

## Fields

- `rule`: the rule that produced it. The id is `diag:<rule>-<n>`. See
  [identifiers](../model.md#identifiers).
- `severity`: `error`, `warning`, `info`, or `none`. `none` is a considered judgment
  recorded but not surfaced. See [judgment](../concepts/judgment.md).
- `subjects`: the node ids or section references it concerns. Subjects must exist. See
  [validation gates](../graph.md#validation-gates).
- `message`: the human-facing text.
- `reasoning`: why this severity was chosen.
- `lifecycle`: `open` or `resolved`.
- `triage`: `null`, `acknowledged`, `suppressed`, or `wontfix`. Set by a human, never
  changed by the compiler.
- `created` and `updated`: build markers.

## Lifecycle and triage

- `open`: the finding stands.
- `resolved`: the condition no longer holds. Set through `resolve_diagnostic` with a
  reason, or by the checks when a deterministic finding clears.
- `triage` is orthogonal to `lifecycle`. A `suppressed` diagnostic stays in the graph and
  keeps being updated, but frontends do not surface it. The compiler shall never
  overwrite a human-set `triage` value.

## Rules catalog

| Source | Rule | Severity | What it catches |
| --- | --- | --- | --- |
| [parsing](../parsing.md#format-handlers) | `unsupported-format` | warning | a matched file with no format handler |
| parsing | `parse-error` | error | a format handler failed on the file |
| parsing | `empty-file` | warning | a matched file with no content |
| [coverage](../reconciler.md#coverage) | `uncovered-section` | warning | a section still `unprocessed` after the build |
| coverage | `suspicious-non-normative` | warning | a `non-normative` section whose text still looks normative |
| review turns | `contradiction` | warning or error | requirements on an entity that cannot all hold |
| review turns | `duplicate-entity` | warning | two entities that look like one concept |
| review turns | `missing-link` | warning | a concept the documents rely on but never define |
| review turns | `ambiguity` | info, warning, or error | a statement open to more than one reading |
| [checks](../reconciler.md#waves) | `unused-entity` | warning | an entity no requirement references |
| checks | `duplicate-requirement` | warning | two requirements on one entity whose statements are near identical |
| checks | `unreachable-entity` | warning | an entity not reachable from the declared roots |
| checks | `unstable-extraction` | warning | a natural key deleted and recreated across recent builds |
| checks | `stale-provenance` | warning | a `quote` that no longer locates in its section |
| [reconciler](../reconciler.md#convergence) | `incomplete-build` | warning | work parked when the build budget ran out |
| [project settings](../project-settings.md) | lint rules | configurable | project-specific lint over the graph |
