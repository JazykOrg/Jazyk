# The dedupe-candidates goal

Goal: judge one pair of entities the name index scored as lookalikes across documents.
Two documents that each introduce "the backend" and "the backend system" leave two
entities for one concept, each extracted without the other in view. Two documents that
each describe a `Product` and a `Product price` leave two entities that share a word and
nothing else. The model reads both in full and merges them, or keeps both with the
reason, and the verdict is remembered so the pair is not asked about again while nothing
about it moves. [`review-entity`](./review-entity.md) judges lookalikes from one changed
entity's side, among its other duties; this goal is the pair-level net over the whole
graph, changed or not.

- Kind: `dedupe-candidates`. Class: GC. Optional: it never blocks convergence and rides
  in the verdict as `optional`.
- Unit: one entity pair. Id: `g:dedupe-candidates:<ent:a>~<ent:b>`, smaller id first.
- Ready when no compile goal is open or parked in either entity's
  [cone](../reconciler.md#cones) ([readiness](../reconciler.md#readiness)). A pair is
  judged after both entities' reviews closed, so the decision sees final definitions and
  final statement sets.

## Created when

Work is computed, never stored ([what needs building](../graph.md#what-needs-building)).
The goal derives from a `lookalike` [change record](../graph.md#change-records)
(`via: lookalike`), computed per derivation from the name index
([derived data](../graph.md#derived-data)) for every pair whose score reaches the
threshold and holds no standing verdict. A pair either side's `distinct_from` names
([fields](../model/entity.md#fields)) holds one: the `note` that let the second name
through the near-duplicate gate told the two apart, and asking again would be asking
the session to repeat itself.

### The lookalike score

The score is deterministic. The store computes it at every commit for every candidate
pair; the model never computes or adjusts it.

- Candidate pairs: two entities in one scope. Different named contexts keep same-named
  concepts apart by declaration and never pair
  ([scope in the natural key](../concepts/scopes.md#scope-in-the-natural-key)). Neither
  entity is an ancestor of the other in the containment tree, and no `composition`,
  `aggregation`, `generalization`, or `instantiation` contribution stands between them:
  the documents already tie those two as two things. Cross-document: the two entities'
  mention documents, taken together, span at least two documents. A pair confined to one
  document is [`review-entity`](./review-entity.md#hints)'s to judge.
- `tokens`: the name and aliases of each entity, lowercased, punctuation stripped, stop
  words dropped, reduced to crude stems (the suffix stage of the stemmer
  [pair selection](../reconciler.md#pairs) uses), as one token set per entity. Each
  token carries a weight, its inverse document frequency over the entity names and
  aliases in the graph, so a token many names share (`service`, `store`) carries less
  signal than one two names share. With `E` the entities in the graph and `n` the
  entities whose token set holds the token:
  `weight = min(1, ln(1 + E / n) / ln(1 + E / 3))`. A token held by three entities or
  fewer (`lookalike-distinctive-holders` 3) weighs 1; the weight falls from there. The
  overlap is the weighted Dice coefficient: twice the weight of the shared tokens over
  the summed weights of both sets. Sets of full-weight tokens give the plain Dice
  coefficient, so two names sharing one distinctive token score as they always did.
- `documents`: the documents mentioning each entity, as one set per entity. The overlap
  is the Jaccard index: the shared documents over the union.
- `score = 0.75 × tokens + 0.25 × documents`. A pair is a candidate when the score
  reaches `0.5`. The weights (`lookalike-token-weight` 0.75, `lookalike-document-weight`
  0.25), the threshold (`lookalike-threshold` 0.5), and the holder count
  (`lookalike-distinctive-holders` 3) are registry constants, not settings
  ([budgets and thresholds](../graph.md#budgets-and-thresholds)). A pair whose score
  reaches the threshold is a candidate.

E.g. "backend" and "backend system", each mentioned in its own document: `tokens` 0.67,
`documents` 0, score 0.5, a candidate. "Product" and "Product price" mentioned in the
same two documents: `tokens` 0.67, `documents` 1, score 0.75, a candidate the judgment
keeps apart. "Order" and "Reorder point": no shared token, never a candidate.

E.g. the weight at work: "Auth Service" and "Billing Service" among sixteen `X Service`
entities in a graph of 25, both mentioned in the same two documents. `service` is held
by 16 of 25 entities: `weight = ln(1 + 25/16) / ln(1 + 25/3) = 0.94 / 2.23 = 0.42`;
`auth` and `billing` weigh 1. `tokens = 2 × 0.42 / (1.42 + 1.42) = 0.3`, `documents`
1, score 0.47, not a candidate. The same two names in a graph of two entities: `service`
is held by both, weight 1, `tokens` 0.5, score 0.63, a candidate. Sixteen services that
share one generic word are not 120 pairs to judge; a shared word two names hold still
is.

```yaml
- kind: lookalike
  subject: ent:backend~ent:backend-system
  via: lookalike
  detail: {score: 0.5, tokens: 0.67, documents: 0, shared: [backend],
           mentions: {ent:backend: [docs/api.md], ent:backend-system: [docs/deploy.md]}}
```

### Verdicts are remembered

A kept pair must not be asked about again on every build: a no-op rebuild derives zero
goals. Each pair kept apart is a decision in the graph,
`graph/decisions/lookalikes/<a~b>.yaml`
([the .jazyk folder](../jazyk-folder.md#graphdecisionskindidyaml)), with the pair's score
at judgment.

- Resolving the goal with `separate` writes the decision with the pair's score at
  judgment. The score is the formula's value over the graph as the session's changeset
  commits (an alias the session added counts). A merge needs no decision: one of the ids
  is gone, and the pair with it.
- A `separate` verdict a [`review-entity`](./review-entity.md#gate) session records in
  its `evidence` writes the same decision for that pair, so a review that kept two
  entities apart with a reason is not asked again by this goal. The decision carries the
  formula's value for the pair whether or not it reaches the threshold, so a pair the
  review kept apart below the threshold is still remembered when it later crosses it
  unchanged. A merge it stages removes the pair outright.
- A decision applies while the pair's score equals the recorded one. A rename, a new
  alias, or a new mention document changes the score, the decision lapses, and the pair
  is judged again over the new evidence; resolving it again rewrites the decision.
- A decision whose pair lost one of its ids (a delete, a merge, a retraction) is deleted
  by the sweep at the same commit.
- The decisions are the only memory. Goal derivation does not read the journal's
  resolved goals for this kind.

```yaml
# Jazyk decision: two similar entities judged to be different things. Written by jazyk.
# yaml-language-server: $schema=https://jazyk.org/schemas/lookalike.json
id: ent:product~ent:product-price
verdict: separate
score: 0.75
```

### Batching

Pairs that share an entity form one locality ([batching](../reconciler.md#batching)):
three lookalikes of one concept are judged in one session, so one survivor absorbs the
rest instead of three sessions each keeping a different id. A merge staged for one pair
previews the `review-entity` it opens on the survivor
([bubbling](../reconciler.md#bubbling)); that review often joins the same session.

## Gate

Merged, or kept with reasoning. At `mark_goal_done`, `evidence` is `merged` or
`separate` (bare or as `{verdict}`; anything else is `bad-evidence`), and the harness
checks:

- `merged`: a staged `merge_entities` names the pair, `keep` and `absorb` in either
  order, with a `reason` (`merge-unbacked` otherwise). The store rewires every
  reference, unions aliases, mentions, and attributes, and leaves a redirect from the
  absorbed id ([mutations](../graph.md#mutations)). A merge that would make the survivor its own
  ancestor is rejected at staging.
- `separate`: no merge is staged on the pair (`merge-staged` otherwise), and the
  justification names what keeps them apart: a part, a role, an instance, a threshold,
  two parents, two contexts. A
  `duplicate-entity` diagnostic on both ids is staged when the doubt is real, never as a
  substitute for the reason. `update_entity` with `add_aliases` may record one entity's
  wording on the other without a merge, when the wording belongs there.

Nothing else closes the goal. At `done`, the per-mutation gates hold
([validation gates](../graph.md#validation-gates)) and a clean batch commits
([commit](../sessions.md#commit)). The survivor's fact hash no longer matches its
review, which opens [`review-entity`](./review-entity.md) on it, which refreshes the definition over the
merged statement set.

The review asymmetry governs the verdict
([the review asymmetry](../concepts/judgment.md#the-review-asymmetry)): a wrong merge
destroys information and everything bound to it, and the graph cannot tell afterwards
that two concepts were ever there; a missed duplicate leaves two nodes and a finding for
the next build. When in doubt, keep both and file. The goal fails (`mark_goal_failed`)
when the documents contradict each other about what the two names mean; the failure is
a `goal-failed` [diagnostic](../model/diagnostic.md) on both entities
([parked and failed](../reconciler.md#parked-and-failed)).

## Hints

The hint computer emits, per goal:

- `load <ent:a>` and `load <ent:b>`: both entities in full.
- `score <s> (tokens <t>, documents <d>; shared: <tokens>)`: the change, with the
  documents that mention one and not the other.
- `load <req>` for the statements that name both entities, when any do: the documents'
  own evidence that the two are one thing or two.
- `load <ent>` for a parent or a child of either, when the two sit in different
  branches of the containment tree: two same-named children of two parents are two
  entities.
- `skill judgment`.
- `merge_entities`, `report_diagnostic`: the tools that resolve the kind.

## What the model sees

The goal block in the [session prompt](../sessions.md#the-prompt) carries the contract
paragraph from [`./prompts/dedupe-candidates.md`](./prompts/dedupe-candidates.md), the
change in one line, the gate in one line, and the hints. E.g.:

```text
- [g:dedupe-candidates:ent:backend~ent:backend-system] optional
  The name index scored these two entities as lookalikes across documents. Load both
  in full. Merge only when they are one concept (a name variant, a synonym, two
  documents describing one thing), keeping the better-established id and saying why.
  Keep both when statements are directly about each as its own thing; a shared word
  proves nothing. When the merge is not certain, keep both and file duplicate-entity.
  Change: score 0.5 (tokens 0.67, documents 0); docs/api.md mentions only ent:backend,
  docs/deploy.md only ent:backend-system.
  Gate: merged, or kept with the reason.
  Hints: load ent:backend; load ent:backend-system; skill judgment.
```

The [judgment skill](../skills/judgment.md) is active from the first round
([skills](../sessions.md#skills)): same versus separate, the better-established id, the
asymmetry, and what a finding the author can act on looks like
([calibration](../concepts/judgment.md#calibration)).

The initially [loaded set](../context.md#the-loaded-set) holds, per goal:

- Both entities in full: `definition`, `aliases`, `scope`, `stereotype`, `parent` with
  its chain, `attributes`, mentions with one parent chain each, and every requirement
  referencing each, with statement and quote ([entity](../model/entity.md#fields)).
  Over-budget statement lists become handles ([policy](../context.md#policy)).
- The two mention-document sets, side by side, the shared documents marked.
- One hop of related entities for each, as stubs, and the relationships between the two
  when any exist.
- The open diagnostics naming either entity.

### Verdicts

- Same concept: a name variant ("backend", "backend system"), a synonym, or two
  documents describing one thing under two wordings. Merge, keeping the id with more
  statements or the earlier mention; the absorbed name survives as an alias, and its
  requirements, attributes, children, and edges follow.
- Separate: a field, part, state, role, threshold, instance, or child concept is its own
  entity when statements are directly about it. "Product price" is not a variant of
  "Product". Two same-named children of two parents are two entities
  ([the natural key under containment](../concepts/identity.md#the-natural-key-under-containment)).
  One name for two concepts in two named contexts stays two entities kept apart by
  `scope`; the session never invents a scope, a parent, or a stereotype to keep them
  apart or to join them.
- Uncertain: keep both and file `duplicate-entity`, subjects both ids, the message
  naming what makes them look alike and what keeps them apart, severity `warning` when
  the session judges them probably one concept and `info` when they are only alike,
  with `reasoning`. When the owner can settle it in one answer, attach a
  [prompt](../model/diagnostic.md#prompts): one `answer` option per direction ("merge
  into ent:backend", "keep both"), `freeform: true`. The unanswered prompt opens the
  blocked [`answer`](./answer.md) goal, and the answer session applies the ruling.

## Tools

The `dedupe-candidates` [toolset](../tools.md#toolsets): the
[read tools](../tools.md#read-tools) (`load`, `expand`, `unload`, `graph_status`, `search`, `read_section`, `get_entity`, `get_view`, `diagnostics`), the [goal tools](../tools.md#goal-tools) (`mark_goal_done`, `mark_goal_failed`, `load_skill`, `done`),
`merge_entities`, `update_entity`, `report_diagnostic` (rule `duplicate-entity`), and
[`report_feedback`](../tools.md#feedback-tool). No requirement tools and no deletes: the
pair is judged as two nodes, and what a merged survivor's statements need is
[`review-entity`](./review-entity.md)'s work at the next commit. See
[write tools](../tools.md#write-tools).
