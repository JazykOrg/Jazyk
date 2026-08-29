# The declare-edges goal

Goal: decide the edges of one requirement that names several entities and declares
none. A multi-entity statement is what makes the graph a graph, and an edge is a claim
the sentence makes ("the order service provides the checkout API"), never something
membership implies. The model reads the statement and its verbatim quote, declares each
pair the sentence ties structurally (directional, typed, with cardinality when a count is
stated), or records that the statement is not structural. Relationships recompute from
the result at commit ([recompute](../model/relationship.md#recompute)), so every arrow a
view draws walks back to a sentence.

- Kind: `declare-edges`. Class: GC. Optional: it never blocks convergence and rides in
  the verdict as `optional`.
- Unit: one requirement. Id: `g:declare-edges:<req>`.
- Ready when no compile goal is open or parked in the requirement's
  [cone](../reconciler.md#cones): the requirement, its entities and their ancestors, the
  views that list it, and the sections that anchor them
  ([readiness](../reconciler.md#readiness)). Extraction and judgment settle the statement
  and its entity list first; the edges are judged once, over the final wording.

## Created when

One [change record](../graph.md#change-records) kind derives the goal: `edges-missing`
(`via: entities`). The commit that creates or revises a requirement writes it when, after
the changeset lands, the requirement lists two or more entities and carries no `edges`
([edges](../model/requirement.md#edges)). E.g.:

```yaml
- id: c414-2
  generation: 414
  mutation: 2
  kind: edges-missing
  subject: req:shop-3
  via: entities
  detail: {entities: [ent:order-service, ent:stock-api], edges: 0}
```

- The record is written by the commit that lands the requirement, never by a scan of the
  graph. A requirement the session judged not structural is not asked about again until
  its `statement` or `entities` change, which writes a fresh record.
- The record clears on its own when the requirement gains an edge by any path: a
  [`review-entity`](./review-entity.md) session adding one, a dual write, a decree.
  Resolving the goal clears it too. Deleting the requirement drops it with its subject.
- A requirement extracted with its edges in the same call never writes the record. The
  [extraction skill](../skills/extraction.md) asks for edges beside every statement, so
  this goal is the net for what extraction left implicit, not the normal path.

### Batching

Requirements that share an entity form one locality
([batching](../reconciler.md#batching)). A batch holds the `declare-edges` goals of one
neighborhood, so the session judges the pairs of one entity together and declares them
consistently: one whole, one set of parts, one interface and its realizer. A burst often
lands in the session that just settled the neighborhood's reviews
([bubbling](../reconciler.md#bubbling)).

## Gate

Edges declared, or a justification saying the statement is not structural. At
`mark_goal_done`, `evidence` is `declared` or `not-structural`, and the harness checks:

- `declared`: a staged `update_requirement` on the target carries `edges` with at least
  one edge. Every edge has a `type`: an untyped edge is refused under this goal, because
  judging the type is the goal. `a` and `b` are distinct and among the requirement's
  `entities`; `cardinality`, when given, is one of `1`, `0..1`, `1..*`, `*`
  ([validation gates](../graph.md#validation-gates)). The call passes neither `section`
  nor `quote`: those re-anchor the provenance to another sentence.
- `not-structural`: nothing is staged on the target, and the justification names the
  sentence and why it relates no pair. The justification is recorded in the journal
  with the resolution ([journal](../graph.md#journal)) and is what stops the goal from
  deriving again.

The `entities` list is not this goal's to change: whether an entity belongs on the
requirement is [`review-entity`](./review-entity.md)'s question. A staged
`update_requirement` that changes `entities`, `statement`, `transition`, or `facets` is
rejected under this goal, naming the field.

At `done`, the per-mutation gates hold and a clean batch commits
([commit](../sessions.md#commit)). The commit recomputes the relationships
([derived data](../graph.md#derived-data)) and the views that show the pair redraw at
the same commit. An edges-only `update_requirement` writes no `requirement-revised`
record: that record means the `statement` or the quote changed in substance
([pairs](../reconciler.md#pairs)). It writes `entity-changed` (`via: edges`) on each of
the requirement's entities, so [`review-entity`](./review-entity.md#created-when) sees
the new edge on its next review.

The goal fails (`mark_goal_failed`) when the sentence is too ambiguous to type honestly
and `dependency` would be a guess about whether any relationship exists at all: the
failure surfaces on the requirement, where the author sees it
([parked and failed](../reconciler.md#parked-and-failed)). A failed optional goal is
recorded and stands.

## Hints

The hint computer emits, per goal:

- `load <req>`: the requirement with its statement, its quote, and its entities as stubs.
- `<n> entities, no edges (g<N>)`: the change.
- `load <ent>` for each entity, so the session sees stereotypes and parents: a whole
  and its part, a realizer and its interface, a type and its instance.
- `related <ent>~<ent>: <type> (<count>)` for each pair already tied by a relationship
  through other requirements, so a declared edge agrees with what the graph holds.
- `skill extraction`.
- `update_requirement`: the tool that resolves the kind.

## What the model sees

The goal block in the [session prompt](../sessions.md#the-prompt) carries the contract
paragraph from [`./prompts/declare-edges.md`](./prompts/declare-edges.md), the change in
one line, the gate in one line, and the hints. E.g.:

```text
- [g:declare-edges:req:shop-3] optional
  This requirement names several entities and declares no edges. Read the statement
  and its quote. Declare each pair the sentence ties structurally with
  update_requirement, passing only id and edges: directional (a acts on b), typed,
  with cardinality when the sentence states a count. When the sentence relates the
  pair but not its kind, declare dependency and say so. When the statement is not
  structural, declare nothing and say why.
  Change: 2 entities (ent:order-service, ent:stock-api), no edges (g414).
  Gate: edges declared, or justification says not structural.
  Hints: load req:shop-3; load ent:stock-api; skill extraction.
```

The [extraction skill](../skills/extraction.md) is active from the first round
([skills](../sessions.md#skills)): edge direction per type, one sentence yielding several
arrows, cardinality from stated counts, and the rule that an edge without a type counts
as the weakest.

The initially [loaded set](../context.md#the-loaded-set) holds, per goal:

- The requirement in full: `statement`, `source` with the verbatim quote, `entities`,
  `transition` and `facets` when present
  ([requirement](../model/requirement.md#fields)).
- Each entity as a stub: name, one definition line, stereotype, parent, edge count. An
  interface-like entity and a containment root are visible as such, and the type follows
  from them ([entity](../model/entity.md#fields)).
- The relationships already standing between any two of the entities, one line each
  with type, direction, cardinality, and contributing requirements
  ([relationship](../model/relationship.md#fields)).
- The section body behind the quote when the budget allows, a handle otherwise
  ([policy](../context.md#policy)).

### Judging the type

- Directional and typed, from the sentence: the whole is `a` of a `composition`, the
  realizer `a` of a `realization`, the instance `a` of an `instantiation`, the dependent
  `a` of a `dependency`, the specific `a` of a `generalization`; `association` for a
  plain link, `aggregation` for a shared part
  ([types](../model/relationship.md#types)).
- Several arrows from one sentence: "A calls B's endpoint" is `A→endpoint` and `A→B`,
  both `dependency`. Both ends of every edge stay in `entities`.
- The weakest honest type. A stronger type the sentence does not state is invented
  structure, and a wrong edge draws a wrong arrow in every view that shows the pair. A
  sentence that relates the pair without saying how yields `dependency`, and the
  justification says so.
- Not structural: the second entity is context, an example, a unit, or a reference. "The
  order total is shown in the customer's currency" states no relationship between the
  order and the customer. Declare nothing.
- Containment. A `composition` declared here must agree with `parent`: when the part's
  `parent` sits in a different branch, the `containment-mismatch`
  [check](../compilation.md#checks) files the disagreement, and setting `parent` is
  `review-entity`'s move ([containment](../model/entity.md#containment)).

## Tools

The `declare-edges` [toolset](../tools.md#toolsets): the
[read tools](../tools.md#read-tools), the [goal tools](../tools.md#goal-tools),
`update_requirement`, and [`report_feedback`](../tools.md#feedback-tool). One write
tool, one field: the goal declares edges and nothing else. No `report_diagnostic`: a
sentence the session cannot type is a failed goal with the reason on the requirement,
not a finding, and the entity's next review files what the author can act on. See
[write tools](../tools.md#write-tools).
