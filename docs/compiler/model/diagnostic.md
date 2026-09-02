# Diagnostic

A diagnostic is a first-class node recording a judgment: a contradiction, an ambiguity,
a conformance finding, a pending decision, an uncovered section. Because diagnostics are
nodes, they keep their id across builds and get edited, not regenerated. Human triage
survives recompilation by construction. Diagnostics record findings and questions;
[goals](../reconciler.md#goal-derivation) carry work and are never stored.

Diagnostics enter the graph through the `report_diagnostic` and `resolve_diagnostic`
[write tools](../tools.md#write-tools). Sessions file the judged rules
(`contradiction`, `duplicate-entity`, `duplicate-requirement`, `missing-link`,
`ambiguity`, `lint`, `decision`, `nonconformant-instance`) and no others. The harness
files the rest: the deterministic [checks](../compilation.md#checks), the commit that
lands a `derived` or `decree` fact, generation, and decompilation.

## Fields

- `rule`: the rule that produced it. The id is `diag:<rule>-<n>`. See
  [identifiers](../model.md#identifiers).
- `severity`: `error`, `warning`, `info`, or `none`. `none` is a considered judgment
  recorded but not surfaced. See [judgment](../concepts/judgment.md).
- `subjects`: the node ids or section references it concerns. Subjects must exist. A
  finding about an attribute subjects its entity and names the attribute in `message`.
  See [validation gates](../graph.md#validation-gates).
- `message`: the human-facing text.
- `reasoning`: why this severity was chosen.
- `lifecycle`: `open` or `resolved`.
- `triage`: `null`, `acknowledged`, `suppressed`, or `wontfix`. Set by a human, never
  changed by the compiler.
- `prompt`: an optional question or proposal attached to the finding. See
  [prompts](#prompts).
- `answer`: the human response, once one arrives. See [answers](#answers).
- `created` and `updated`: generation markers.

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
  - `edit`: a suggested edit, `{doc, section, old_text, new_text}`, the
    same shape the [dual-write tools](../../frontends/acp.md#dual-write-tools) use.
    `old_text` non-empty: `new_text` overwrites it; `old_text` empty: `new_text` is
    appended to the end of the section's body. Choosing an `edit` is
    deterministic: no model runs.
  - `answer`: a prefilled reply. Choosing it hands the reply to the model.
- `freeform`: whether a typed reply is accepted (handled like an `answer`).

Who writes prompts: [sessions](../sessions.md) (`rejudge-pair`, `review-entity`,
`abstract-entity`, and chat sessions) attach them through `report_diagnostic`
([write tools](../tools.md#write-tools)) and maintain them the same way: re-reporting
the finding is a natural-key upsert and carries the new prompt. `update_diagnostic`
replaces the question alone; it is served on the chat path and the raw MCP servings
([toolsets](../tools.md#toolsets)), never in a goal session's toolset. The
deterministic [checks](../compilation.md#checks) attach them mechanically where the
resolution is enumerable (`pinned-fact-drift`, `unstable-derivation`); the commit that
lands a `derived` or `decree` fact attaches the
[ratification proposal](#ratification-proposals). A prompt is optional; most
diagnostics carry none. A diagnostic with an unanswered prompt is the `prompt-unanswered`
change and opens the blocked [`answer` goal](../goals/answer.md), which rides in the
verdict's `blocked` count; an info-severity diagnostic whose rule is not `decision`
derives no goal from its prompt (an observation's prompt is advice, not a standing
question).

Gates: an `edit` option's `old_text` must locate in its section
(whitespace-insensitively) when it is non-empty, `label` is required, and `edit` and
`answer` are mutually exclusive per option.
The `section` takes either reference form: `/ref` or the full `doc.md#/ref` the loaded
set displays.

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

- Choosing an `edit` option applies it as a dual write, journaled as `answer` (the entry
  carries the prose replacement and the graph mutation), or as `ratify` for a
  ratification proposal; `dual-write` is the kind the
  [chat dual-write tools](../../frontends/acp.md#dual-write-tools) land. See
  [journal](../graph.md#journal). The file changes on disk, the section hashes
  are absorbed in the same changeset (no recompile owed), and the diagnostic resolves
  immediately with the option's label as the reason (`status: applied`). A requirement
  whose own quoted sentence the edit rewrites is re-anchored in the same changeset, its
  statement mechanically updated when the replaced text appears in it verbatim. Anchors
  the engine cannot re-anchor mechanically go stale normally and the next build
  reconciles them.
- Choosing an `answer` option or replying freeform records the text
  (`status: handling`) and applies it in an answer session over
  [ACP](../../frontends/acp.md#answer-sessions) (journal entry `answer`): in a chat
  session the session's own agent acts on it with its tools; elsewhere jazyk spawns an
  answer session. The answer session resolves the diagnostic (or updates its prompt and
  leaves it open); `status` moves to `handled`, or `failed` with the error. The retract
  option of a ratification proposal is the exception: it is deterministic, and no model
  runs.
- An `answer` is a human record, like `triage`: the compiler never overwrites it, and
  a re-detected condition on a node that carries one is not re-asked. A rejected
  suggestion stays rejected across rebuilds.

## Lifecycle and triage

- `open`: the finding stands.
- `resolved`: the condition has cleared. Set through `resolve_diagnostic` with a
  reason, or by the checks when a deterministic finding clears. A ratification proposal
  resolves only through accept or retract; `resolve_diagnostic` on it is refused.
- `triage` is orthogonal to `lifecycle`. A `suppressed` diagnostic stays in the graph and
  keeps being updated, but frontends do not surface it. The compiler shall never
  overwrite a human-set `triage` value. A triage change is journaled as a `triage`
  entry. See [journal](../graph.md#journal).
- A judged diagnostic whose subjects are all gone from the graph is resolved by the
  checks and journaled as `settle-diagnostics`.

## Rules catalog

| Source | Rule | Severity | What it catches |
| --- | --- | --- | --- |
| [parsing](../parsing.md#format-handlers) | `unsupported-format` | warning | a matched file with no format handler |
| parsing | `parse-error` | error | a format handler failed on the file |
| [coverage](../compilation.md#coverage) | `uncovered-section` | warning | a section with a body of its own still `unprocessed` after the build |
| coverage | `suspicious-non-normative` | warning | a `non-normative` section whose text still looks normative |
| [`rejudge-pair`](../goals/rejudge-pair.md), [`review-entity`](../goals/review-entity.md) sessions | `contradiction` | warning or error | requirements on an entity that cannot all hold |
| `review-entity`, [`dedupe-candidates`](../goals/dedupe-candidates.md) sessions | `duplicate-entity` | warning | two entities that look like one concept |
| `rejudge-pair`, `review-entity` sessions, checks | `duplicate-requirement` | warning or info | warning: the same obligation recorded twice; info: the same fact intentionally restated in different documents (both kept) |
| `review-entity` sessions | `missing-link` | warning | a concept the documents rely on but never define; a dead file link is `broken-link`'s finding, never this rule's |
| sessions | `ambiguity` | info, warning, or error | a statement open to more than one reading |
| sessions, chat | `decision` | info, warning, or error | a choice the documents leave open, the `prompt` carrying the question and its options; unanswered, it opens the blocked `answer` goal |
| [`conform-instance`](../goals/conform-instance.md) sessions, checks | `nonconformant-instance` | warning | an instance that does not conform to its type: an attribute name the type does not declare (mechanical), a value or a link the type's statements rule out (judged) |
| `review-entity` sessions, [docsgen](../../consumers/docsgen.md#plain-english-lint) | `lint` | configurable | a project lint rule violated |
| [checks](../compilation.md#checks) | `unused-entity` | warning | an entity no requirement references |
| checks | `unreachable-entity` | warning | an entity not reachable from the declared roots |
| checks | `stale-provenance` | warning | a `quote` that fails to locate in its section |
| checks | `unstable-extraction` | warning | a natural key deleted and recreated across recent builds |
| checks | `unstable-derivation` | warning | a natural key a GC commit and a compile commit flip back and forth; the pair parks, both justifications ride in `reasoning`, and the `prompt` asks which direction holds |
| checks | `section-too-large` | warning | a section body over 6000 chars; split it |
| checks | `doc-too-large` | warning | a document over 40 sections; split it |
| checks | `empty-file` | warning | a matched file with no content |
| checks | `broken-link` | warning | a relative link to a `.md` file whose target does not exist; links that escape the project root are ignored |
| checks | `pinned-fact-drift` | warning | a literal the docs pin (a path, id, or value in a code span) that the requirement's bound files never mention |
| checks | `unjustified-fact` | error | a fact or rendered element whose provenance walk ends in neither a verbatim quote in a live section nor a `derived` or `decree` fact with live upstream nodes and an open ratification proposal |
| checks | `unplaced-behavior` | info | a `behavior` requirement in no flow view and excluded from none |
| checks | `unrepresented-failure-mode` | info | a `failure-mode` requirement in no flow view and excluded from none, so no branch represents it |
| checks | `containment-mismatch` | warning | a `composition` edge whose part sits in a different branch of the containment tree than its whole |
| checks | `unreachable-state` | warning | a state no path from the initial state reaches |
| checks | `dead-end-state` | info | a state with no outgoing transition: the final state, or a requirements gap |
| checks | `nondeterministic-transition` | warning | two transitions out of one state on one trigger with overlapping guards |
| checks | `unhandled-event` | info | a state and a trigger the subject's requirements name with no transition out of that state on that trigger; silent under two transitions |
| checks | `provider-missing` | warning | an interface-like entity something depends on with no `realization` toward it |
| checks | `provider-ambiguous` | warning | an interface-like entity with more than one realizer |
| checks | `quality-unmeasured` | warning | a `quality` facet without a `measure` |
| [reconciler](../compilation.md#convergence) | `incomplete-build` | warning | goals parked when a build budget ran out |
| the store | `ratification-pending` | warning | a `derived` or `decree` fact with no sentence in the documents; the `prompt` proposes one |
| [generation](../../consumers/gen.md) | `invented-choice` | error, warning, or info | a choice generation made that the documents leave open, graded by the scope of the invention: error when the invention is the product, warning for an unspecified behavior, suppressible info for an unspecified detail |
| [decompilation](../../consumers/decompile.md#ratification) | `unratified` | info | a drafted document no human has edited since the draft landed |
| [project settings](../project-settings.md) | lint rules | configurable | project-specific lint over the graph |

## Ratification proposals

A `derived` or `decree` fact has no sentence in the documents behind it. The commit that
lands such a fact files one `ratification-pending` diagnostic on it, mechanically, and
writes the `provenance-pending` change record the blocked
[`ratify` goal](../goals/ratify.md) derives from. The diagnostic is the proposal; the goal
is the count on the board. One proposal per fact, open exactly while the fact stands with
non-quote provenance.

- `subjects`: the fact's node. For an attribute, the entity, the `message` naming the
  attribute.
- `severity`: `warning`.
- `reasoning`: for a derived fact, the session's reasoning and its `from`; for a decree,
  the author, the time, and the note.
- `prompt`: the question, one `edit` option that inserts the proposed sentence, and one
  `answer` option labeled retract, with `freeform: true`. A freeform reply that rewords
  the sentence is the accepted sentence.

E.g.:

```yaml
diag:ratification-pending-2:
  rule: ratification-pending
  severity: warning
  subjects: [ent:order-pricing]
  message: "ent:order-pricing is derived and no document states it."
  reasoning: "abstract-entity, g420: req:orders-3 and req:orders-9 share one concern, pricing, separable from the order itself."
  lifecycle: open
  prompt:
    question: "Should docs/orders.md state the pricing module?"
    options:
      - label: "Insert into docs/orders.md /orders/service"
        edit: {doc: docs/orders.md, section: /orders/service, old_text: "",
               new_text: "The order service contains a pricing module that computes totals and applies discounts."}
      - label: "Retract"
        answer: "retract"
    freeform: true
  created: g420
  updated: g420
```

The sentence and its target:

- The sentence is the fact's own text: a requirement's `statement`; an entity's `name`
  and `definition`; an attribute as a sentence naming the entity, the attribute, and its
  type or value. A session that stages a derived fact writes it to be read as prose,
  because the statement is the proposal.
- A decree on a fact that had a quote targets the fact's former source section, and
  `old_text` is the former quote: the edit overwrites the sentence.
- A new fact targets the section that sources most of its `from` facts (a sub-entity: the
  section that defines its parent; a decree with no upstream: the first mention of its
  first entity), and `old_text` is empty: the sentence is appended to the section's body.
- The target is always an existing section. An oversized section or document keeps its
  `section-too-large` or `doc-too-large` advice to split; the proposal never creates a
  document, and it follows the section wherever a split moves it.

The two ways out:

- Accept. Choosing the `edit` is a dual write journaled as `ratify`: the sentence lands in
  the document (as the human edited it), the section hashes are absorbed in the same
  changeset, and in the same changeset the fact's provenance flips to `quote` with the
  landed sentence as the quote (`source` on a requirement, a mention on an entity, the
  attribute's `provenance`). The diagnostic resolves with `status: applied`, the change
  record clears, the goal is gone. No session runs. Downstream goals derive from the
  graph change as usual.
- Retract. Choosing the retract option runs `retract_decree`
  ([mutations](../graph.md#mutations)) with reason `retracted`, journaled as `ratify`,
  deterministically. A node created by derivation or decree is deleted. A field decreed
  over a quoted fact returns to the prior value and source recorded in the decree's
  journal entry, so its provenance is `quote` again. A deletion's cone opens the usual
  goals (`retrace` on the views and instances that referenced it, `review-entity` on the
  entities whose facts moved). A deleted entity's requirements and mentions move to its
  `parent` first, so nothing is orphaned. See
  [the human path](../goals/ratify.md#the-human-path).

The owner may also write the sentence anywhere in their own words. The next build's
`reconcile-section` matches it by natural key and refreshes the fact's provenance to
`quote` ([requirement identity](./requirement.md#identity),
[entity fields](./entity.md#fields)); the proposal resolves and the goal closes. The
proposal is a convenience, never the only door.

Docsgen renders every open proposal on the subject's page and groups the proposals of one
target document together ([docsgen](../../consumers/docsgen.md)); the LSP shows the
diagnostic at the target section with `Apply:` and `Answer:` code actions; the GUI board
shows the goal blocked with the reason `awaiting ratification`. A `derived` or `decree`
fact with no open proposal is `unjustified-fact`.
