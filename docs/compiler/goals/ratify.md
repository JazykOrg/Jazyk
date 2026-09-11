# The ratify goal

`ratify` is where an invented fact meets its prose. A fact whose provenance is
`derived` (synthesized by a session from upstream nodes) or `decree` (authored by a
human directly on the graph) has no sentence in the documents behind it. The goal
stands until one does: the owner accepts the proposed sentence and the fact flips to
`quote`, or the fact is retracted. No session runs it. It keeps the report honest: a
build with open ratifications is `converged, 2 blocked`, never silently done. See
[provenance](../model.md#provenance).

- Class: compile. Readiness tier 3 ([the catalog](../reconciler.md#the-catalog),
  [readiness](../reconciler.md#readiness)). Blocked on a human: it is never ready for
  a session, it never blocks convergence, and it rides in the verdict as a count.
- Unit: one fact: an entity, a requirement, or an attribute. Goal id
  `g:ratify:<node id>`; an attribute's goal targets its entity and names the
  attribute in `change`.
- Skill: none. No contract paragraph: `prompts/` carries no `ratify.md`.

## Created when

The goal derives from a `provenance-pending` [change record](../graph.md#change-records),
written by the commit that lands a fact with `derived` or `decree` provenance:

- A GC session stages sub-entities and the statements it writes for them
  ([`abstract-entity`](./abstract-entity.md)): each new fact carries
  `derived: {from, reasoning}`.
- A human edits a derived fact, adds a fact with no prose behind it, or edits a
  quoted fact without accepting a proposed rewrite: `edit_fact` records a `decree`
  ([edit paths](../compilation.md#edit-paths),
  [dual-write tools](../../frontends/acp.md#dual-write-tools)).

E.g.:

```yaml
- id: c420-4
  generation: 420
  mutation: 4
  kind: provenance-pending
  subject: ent:order-pricing
  via: provenance
  detail: {provenance: derived, from: [ent:order, req:orders-3, req:orders-9],
           proposal: diag:ratification-pending-2}
```

What derives no goal:

- Views. Default views are derived data recomputed at commit, and a curated view's
  justification closes through its members; there is no sentence for a view to gain.
- Per-node limit bumps (`limits` with decree provenance,
  [the limits registry](../graph.md#limits)): a compiler setting, not a fact about
  the subject.
- Derived data (relationships, state machines): recomputed from quoted requirements.

The goal exists exactly while the fact stands with non-quote provenance. Flipping the
provenance to `quote`, or deleting the fact, clears the record.

## Gate

The fact's provenance is `quote` (the accepted or hand-written sentence locates
verbatim in a live section), or the fact is gone. Nothing else closes it. The
deterministic checks agree: a `derived` or `decree` fact without an open proposal is
an `unjustified-fact` diagnostic ([checks](../compilation.md#checks)), and the open
`ratify` goal with its proposal is what keeps such a fact justified meanwhile.

## Hints

Rendered for the human, on the goal card and in `jazyk explain`:

- The proposed sentence, its target document and section, and the docsgen page that
  carries the proposal.
- For a derived fact: the upstream facts (`from`) and the session's reasoning.
- For a decree: the author, the time, and the note.
- The two ways out: accept (`Apply:` the edit) or retract.

## What the model sees

No session claims a `ratify` goal, and no contract paragraph exists for it. A session
that loads the fact sees it like any other node, with its provenance kind in the
loaded set, and treats a `derived` fact as invented until ratified: judgment goals
prefer the quoted side of a contradiction. A session may stage a better statement on
a derived fact (the statement is the proposal), which refreshes the proposal without
closing the goal.

## The proposal

Every pending fact carries one proposal, a `ratification-pending` diagnostic with a
`prompt` ([ratification proposals](../model/diagnostic.md#ratification-proposals)),
filed mechanically by the commit that lands the fact. Docsgen composes the sentence
and its target and renders the proposal on the subject's page
([ratification proposals](../../consumers/docsgen.md#ratification-proposals)):

- The sentence. When the fact's author staged one, it stands: a session that stages a
  derived fact writes its statement to be read as prose, because the statement is the
  proposal; a decree carries the human's `note`. Otherwise docsgen composes it from
  the fact: a requirement's `statement`; an entity's name and `definition` as one
  sentence; an attribute as the entity, the attribute, and its type or value.
- The target. For a derived fact, the section quoted by most of its `from` nodes; for
  a decree, the first entity's first mention; always an existing section (an oversized
  document keeps its `doc-too-large` advice to split; the proposal never creates a
  document). A decree
  that replaced a quoted fact keeps the fact's former source section, and the `edit`
  rewrites the former quote (`old_text` is the former quote), so accepted prose never
  stands beside the sentence it overrules.
- The options: the `edit` (accept), and an `answer` option labeled retract. Freeform
  is accepted: a reply that rewrites the sentence is the accepted sentence.

One goal and one proposal per fact. Docsgen groups the pending proposals of one
target document together in its ratification report, so a reviewer reads them as one
draft.

## The human path

### Accept

Choosing the `edit` (an [LSP code action](../../frontends/lsp.md#capabilities), the
[GUI questions panel](../../frontends/gui.md#questions),
[`answer_diagnostic`](../../frontends/acp.md#questions-in-chat) in chat, or
[`jazyk answer`](../../frontends/cli.md#jazyk-answer) in the terminal) is a dual
write, journaled as `ratify` ([journal](../graph.md#journal)):

- The sentence lands in the document, edited by the human when they changed it.
- The section hashes are absorbed in the same changeset, so the edit does not dirty
  the document it just changed.
- The fact's provenance flips to `quote: {doc, section, quote}` with the landed
  sentence as the quote (`source` on a requirement); an edited sentence also becomes
  a requirement's `statement`.
- The diagnostic resolves (`status: applied`), the change record clears, the goal is
  gone. No session runs.

Downstream goals derive from the graph change as usual: a requirement whose
statement the human reworded opens `rejudge-pair` and `bind` on it.

### Retract

Choosing retract runs `retract_decree` with reason `retracted`, journaled as
`ratify`. A fact created by decree or derivation is deleted, and with it the facts
derived from it (those whose `from` names it); a field decreed over a formerly quoted
fact returns to the prior value and source recorded in the decree's journal entry
([mutations](../graph.md#mutations)), so its provenance is `quote` again. For a
decree this undoes the human's edit; for a derived fact it undoes the session's
invention. No model runs. Quoted requirements that referenced a retracted entity are re-pointed to
its `parent` in the same changeset, so no quoted fact is orphaned and the parent's
`review-entity` goal judges the result; an entity with no parent cannot be retracted
while quoted requirements reference it, and the refusal names them. A retracted
entity that holds children (a [grouping](../concepts/levels.md#groupings)) dissolves
exactly as `dissolve_entity` would ([write tools](../tools.md#write-tools)): its
children reparent to its parent (parentless when the grouping was top-level), the
entity tombstones with a redirect to that parent, and the `ratify` entry journals
the dissolution as a `dissolve_entity` mutation with the children moved, so the
reparent flip replays from the journal. The deletion's
cone opens the usual goals: `retrace` on the views and instances that referenced the
fact, `review-entity` on the entities whose facts moved and on the parent that
regained the children. The next build re-derives
whatever the documents still support. A retract the store refuses lands nothing: the
changeset is all or nothing, no answer is recorded on the proposal, and the refusal
is the reply, so the proposal stays answerable once the reason is gone.

### Write the sentence by hand

The owner may ignore the proposal and write the fact into the documents in their own
words, anywhere. The next build's `reconcile-section` extracts the sentence. When the
extraction's natural key matches the pending fact (a derived or decreed requirement
keys on its statement, an entity on name and scope,
[identity](../model/requirement.md#identity)), the upsert refreshes its provenance to
`quote` and the goal closes. When the wording differs, the quoted statement lands
beside the derived one and [`rejudge-pair`](./rejudge-pair.md) judges the pair: the
derived statement is the worse-sourced duplicate, and its deletion closes the goal.
The proposal is a convenience, never the only door.

## On status surfaces

- The verdict counts it: `converged, 2 blocked` or `incomplete: ... 2 blocked ...`
  ([convergence](../compilation.md#convergence)). Blocked goals never block
  convergence; they ride as counts.
- `jazyk compile` prints `N blocked` in its board summary line; `jazyk status` shows
  the board counts; `jazyk watch` prints one line when the goal opens and one when it
  resolves; `jazyk explain g:ratify:<id>` says which commit produced the fact, what
  the proposal is, and where it lands ([CLI](../../frontends/cli.md)).
- The GUI board shows a card in the compile column, blocked with the reason
  `awaiting ratification`, linking to the proposal; the inspector's justification
  walk ends at the open proposal instead of a quote ([GUI](../../frontends/gui.md)).
- The LSP shows the `ratification-pending` diagnostic at the target section with
  `Apply:` and `Answer:` code actions; docsgen renders the proposal on the entity's
  page; chat lists it under `/questions` and in the session-start summary.
- `goals({})` over MCP lists it as `blocked` with the reason.

## Tools

No session tools. The human's tools: the `edit` and `answer` options of the proposal
through the LSP, the GUI, chat's `answer_diagnostic`, or `jazyk answer`; `edit_fact` and
`retract_requirement` in chat ([chat tools](../tools.md#chat-tools)); the GUI
inspector's decree edits. Every path lands the same `ratify` journal entry.
