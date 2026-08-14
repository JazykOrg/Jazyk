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
- `prompt`: an optional question or proposal attached to the finding. See
  [prompts](#prompts).
- `answer`: the human response, once one arrives. See [answers](#answers).
- `created` and `updated`: build markers.

## Prompts

A diagnostic can carry a `prompt`: a question for the document owner, with optional
suggested resolutions. The prompt lives on the node, so it survives rebuilds exactly
like the finding itself, and every frontend renders the same question from the same
place.

```yaml
prompt:
  question: "orders.md says 21 days, payment.md says 30. Which one holds?"
  options:
    - label: "21 days; fix payment.md"
      edit: {doc: docs/payment.md, section: /payment/rules,
             old_text: "within 30 days", new_text: "within 21 days"}
    - label: "30 days; fix orders.md"
      edit: {doc: docs/orders.md, section: /order/lifecycle,
             old_text: "within 21 days", new_text: "within 30 days"}
    - label: "Both are right; they cover different order kinds"
      answer: "The bounds differ on purpose; make each document name which orders it covers."
  freeform: true
```

- `question`: one sentence, addressed to a person.
- `options`: up to 4 choices. Each has a `label` and exactly one of:
  - `edit`: a suggested edit, the same shape the
    [dual-write tools](../../frontends/acp.md#dual-write-tools) use. Choosing it is
    deterministic: no model runs.
  - `answer`: a prefilled reply. Choosing it hands the reply to the model.
- `freeform`: whether a typed reply is accepted (handled like an `answer`).

Who writes prompts: [review turns](../turns.md#task-types) and chat sessions attach
them through `report_diagnostic` and edit them through `update_diagnostic`
([write tools](../tools.md#write-tools)); deterministic
[checks](../reconciler.md#waves) attach them mechanically where the resolution is
enumerable. A prompt is optional; most diagnostics carry none.

Gates: an `edit` option's `old_text` must locate in its section
(whitespace-insensitively) when the prompt is staged, `label` is required, and `edit`
and `answer` are mutually exclusive per option. The `section` takes either reference
form: `/ref` or the full `doc.md#/ref` the packs display.

## Answers

Answering is a human act, through any frontend ([LSP code actions](../../frontends/lsp.md#capabilities),
[chat sessions](../../frontends/acp.md#questions-in-chat), the
[GUI](../../frontends/gui.md#questions)). The response lands on the node:

```yaml
answer:
  choice: 2        # option index, null for a freeform reply
  text: "The bounds differ on purpose; ..."
  status: handling # applied | handling | handled | failed
```

- Choosing an `edit` option applies it as a dual write: the file changes on disk, the
  section hashes are absorbed in the same changeset (no recompile owed), and the
  diagnostic resolves immediately with the option's label as the reason
  (`status: applied`). A requirement whose own quoted sentence the edit rewrites is
  re-anchored in the same changeset, its statement mechanically updated when the
  replaced text appears in it verbatim. Anchors the engine cannot re-anchor
  mechanically go stale normally and the next build reconciles them.
- Choosing an `answer` option or replying freeform records the text
  (`status: handling`) and invokes the model over [ACP](../../frontends/acp.md#answer-sessions):
  in a chat session the session's own agent acts on it with its tools; elsewhere
  jazyk spawns an answer session. The handling turn resolves the diagnostic (or
  updates its prompt and leaves it open); `status` moves to `handled`, or `failed`
  with the error.
- An `answer` is a human record, like `triage`: the compiler never overwrites it, and
  a re-detected condition on a node that carries one is not re-asked. A rejected
  suggestion stays rejected across rebuilds.

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
| [coverage](../reconciler.md#coverage) | `uncovered-section` | warning | a section still `unprocessed` after the build |
| coverage | `suspicious-non-normative` | warning | a `non-normative` section whose text still looks normative |
| review turns | `contradiction` | warning or error | requirements on an entity that cannot all hold |
| review turns | `duplicate-entity` | warning | two entities that look like one concept |
| review turns, checks | `duplicate-requirement` | warning or info | warning: the same obligation recorded twice; info: the same fact intentionally restated in different documents (both kept) |
| review turns | `missing-link` | warning | a concept the documents rely on but never define |
| review turns | `ambiguity` | info, warning, or error | a statement open to more than one reading |
| [checks](../reconciler.md#waves) | `unused-entity` | warning | an entity no requirement references |
| checks | `section-too-large` | warning | a section body over the configured size; split it |
| checks | `doc-too-large` | warning | a document with more sections than the configured cap; split it |
| checks | `empty-file` | warning | a matched file with no content |
| checks | `broken-link` | warning | a relative link to a `.md` file whose target does not exist; links that escape the project root are ignored |
| checks | `entity-too-dense` | info | an entity's requirement count approaches the generation ceiling; consider subsections |
| checks | `unreachable-entity` | warning | an entity not reachable from the declared roots |
| checks | `unstable-extraction` | warning | a natural key deleted and recreated across recent builds |
| checks | `stale-provenance` | warning | a `quote` that no longer locates in its section |
| checks | `pinned-fact-drift` | warning | a literal the docs pin (a path, id, or value in a code span) that the requirement's bound files never mention |
| [reconciler](../reconciler.md#convergence) | `incomplete-build` | warning | work parked when the build budget ran out |
| [project settings](../project-settings.md) | lint rules | configurable | project-specific lint over the graph |
