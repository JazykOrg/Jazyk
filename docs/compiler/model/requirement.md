# Requirement

A requirement is one atomic obligation about one or more [entities](./entity.md), written
as a free-form `statement`. Requirements are the primary semantic content of the graph:
entities exist because requirements need them, [relationships](./relationship.md) derive
from requirement `edges`, [state machines](./state-machine.md) derive from requirement
transitions, and flow [views](./view.md) are ordered sets of requirements.

E.g.:

```
Each user email is unique.
When the customer checks out, the shop empties the shopping cart.
When payment succeeds, the order becomes paid.
```

The model writes the statement in whatever wording carries it best. The guidance points at
clarity (specific, testable, entity-anchored, one obligation per statement) without
prescribing a syntax. See [statements](../concepts/statements.md).

## Fields

- `statement`: the obligation, free-form text. One statement, one obligation. See
  [granularity](../concepts/statements.md#granularity).
- `entities`: the entity ids the statement is about. At least one. Multi-entity statements
  are encouraged: they are what makes the graph a graph.
- `edges`: `[{a, b, type?, cardinality?}]`, the relationships this statement causes. See
  [edges](#edges).
- `transition`: `{subject, from, to, trigger?, guard?}`, present when the statement
  describes a state change. See [transition](#transition).
- `sources`: a non-empty list of `{doc, section, quote}` locations for an obligation
  extracted from prose. Each quote is verbatim, located by whitespace-insensitive
  string search, and may span consecutive sentences within its section.
- `provenance`: `{derived: {from, reasoning}}` or `{decree: {author, at, note}}` for a
  requirement with no sentence behind it yet. A requirement has exactly one of non-empty `sources` or
  `provenance`. See [provenance](../model.md#provenance).
- `confidence` and `reasoning`.
- `created` and `updated`: generation markers.

E.g.:

```yaml
req:shop-3:
  statement: When checkout succeeds, the order service reserves stock through the stock API.
  entities: [ent:order-service, ent:stock-api]
  edges: [{a: ent:order-service, b: ent:stock-api, type: dependency}]
  sources:
  - {doc: docs/shop.md, section: /shop/checkout,
     quote: "When checkout succeeds, the order service reserves stock through the stock API."}
  confidence: 0.9
  created: g12
  updated: g12
```

## Edges

Edges are plural, directional, typed, and carry cardinality:

- Plural: one sentence, several arrows. "System A calls system B's API endpoint" yields
  `A→endpoint dependency` and `A→B dependency` at once.
- Directional: `a` acts on `b`. The whole is `a` of a `composition`, the realizer is `a`
  of a `realization`, the instance is `a` of an `instantiation`, the dependent is `a` of a
  `dependency`, the specific is `a` of a `generalization`.
- Typed: `type` is one of the [relationship types](./relationship.md#types). An edge
  without `type` contributes `dependency`, the weakest type. The
  [`declare-edges` goal](../goals/declare-edges.md) is where the type gets judged.
- Cardinality: `cardinality` is a UML multiplicity, `*`, a count (`1`, `4`), or a range (`0..1`, `1..*`, `2..3`), the multiplicity of `b`
  as seen from `a`. "A shopping cart holds one or more order items" yields
  `{a: ent:shopping-cart, b: ent:order-item, type: composition, cardinality: "1..*"}`.

Only entities listed in `entities` may appear as `a` or `b` (a gate). Requirements are the
only source of edges: a diagram arrow or a structural sentence ("A is part of B") is
captured as a requirement, which yields the edge, so every arrow is backed by a statement
and provenanced through it. On every commit the store recomputes relationships from all
edges. See [recompute](./relationship.md#recompute).

A requirement with two or more entities and no edges is the `edges-missing` change and
opens the optional [`declare-edges` goal](../goals/declare-edges.md). The session declares
the edges or records that the statement is not structural.

## Transition

`transition: {subject, from, to, trigger?, guard?}` says the statement describes the
subject entering a state:

- `subject`: the entity whose state changes. It is listed in `entities` (a gate).
- `from` and `to`: state names, free-form, compared after trimming, lowercasing, and
  collapsing whitespace. `from` may equal `to`: the subject stays put and the event is
  handled.
- `trigger`: the event. `guard`: the condition under which the transition fires.

E.g.:

```yaml
statement: When payment succeeds, the order becomes paid.
entities: [ent:order]
transition: {subject: ent:order, from: placed, to: paid, trigger: payment succeeds}
```

Transitions are the source of the derived [state machine](./state-machine.md): the store
recomputes the subject's machine on every commit, and the machine checks run on it. A
transition supplies lifecycle and interaction evidence for authored diagrams.

## Observable criteria

A requirement needs no classification. Its statement carries the obligation.
Concrete bounds, observable conditions, outcomes and test shape belong in
[criteria](./criterion.md). Edges and transitions supply graph structure and
interaction evidence without classifying the statement.

## Identity

- A requirement is one obligation, independent of the sentences that state it.
  Its immutable id survives statement edits, source attachment, and source removal.
- The quoted natural key is the punctuation-insensitive statement within its entity
  scopes. Source locations do not participate. Operators, signs, and decimals remain
  significant. An upsert unions sources on the matching requirement.
- Search existing requirements before creating one. Semantic equivalents with different
  wording attach their source with `add_requirement_source` to the existing id.
  A stale anchor in the same section identifies an edited obligation before minting
  another id. Ambiguous matches require an explicit id, never an arbitrary merge.
- Derived requirements key on the normalized statement within their sorted `from` set.
  Decreed requirements key on the normalized statement. Ratification preserves the id
  and replaces provenance with non-empty sources.
- Statement hashes depend only on normalized obligation text. Source attachment,
  removal, relocation, and quote formatting do not invalidate pair decisions,
  criterion `statement_hash`, ledger `hashes.requirement`, or test names. Changing
  the obligation invalidates them. Review fact hashes include all source locations.
- `remove_requirement_source` removes exactly the named location. Reconciliation of
  section A removes only A's obsolete anchors. Removing the last source deletes the
  requirement and its criteria. Derived and decreed facts have no quote sources.
- `merge_requirements` unions sources, entities and edges, reparents criteria,
  and redirects the loser to the survivor. Conflicting transitions require an explicit
  reconciliation before merging. Ledger rows, views, diagnostics, derived references,
  and pair decisions follow redirects. Changed obligation hashes retain no false pass.
