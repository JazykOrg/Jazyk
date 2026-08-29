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
- `facets`: `[{facet, reasoning, measure?}]`, the judged facets of the statement. See
  [facets](#facets).
- `source`: `{doc, section, quote}` for a requirement extracted from prose. The `quote` is
  the verbatim source sentence, located by whitespace-insensitive string search.
- `provenance`: `{derived: {from, reasoning}}` or `{decree: {author, at, note}}` for a
  requirement with no sentence behind it yet. A requirement has exactly one of `source` or
  `provenance`. See [provenance](../model.md#provenance).
- `confidence` and `reasoning`.
- `created` and `updated`: generation markers.

E.g.:

```yaml
req:shop-3:
  statement: When checkout succeeds, the order service reserves stock through the stock API.
  entities: [ent:order-service, ent:stock-api]
  edges: [{a: ent:order-service, b: ent:stock-api, type: dependency}]
  facets: [{facet: behavior, reasoning: A step the order service performs on an event.}]
  source: {doc: docs/shop.md, section: /shop/checkout,
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
- Cardinality: `cardinality` is one of `1`, `0..1`, `1..*`, `*`, the multiplicity of `b`
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
transition statement usually also carries a `behavior` facet, which places it in a flow.

## Facets

`facets: [{facet, reasoning, measure?}]`. A facet is a judgment recorded at extraction,
with its reasoning. Nothing derives a facet from the wording; the model states it. A
requirement may carry several facets, or none.

- `behavior`: the subject does something, in response to an event or as a step. Behavior
  requirements cluster into flow views. A behavior requirement placed in no flow view and
  excluded from none draws `unplaced-behavior`.
- `constraint`: an invariant the subject holds.
- `failure-mode`: what happens when something goes wrong. A failure-mode requirement gives
  a flow its branches. One represented in no branch draws `unrepresented-failure-mode`.
- `quality`: a quality attribute (performance, capacity, availability). `measure` is
  stated only on `quality`: the measurable bound, e.g. `2 seconds`. A `quality` facet
  without `measure` draws `quality-unmeasured`. A time measure on a subject that has a
  state machine is what a curated [`timing` view](./view.md#kinds) reads. Nothing
  derives a timing view; there is no default one.

Consumers read facets: the default flow views, the flow placement checks, and test shaping
in [generation](../../consumers/gen.md).

## Identity

- The natural key is the source section plus the punctuation-insensitive statement text,
  so a punctuation or spacing edit to a sentence matches its existing requirement and
  refreshes `statement` and `quote` in place. `upsert_requirement` keys on it. A retry
  lands on the existing node. An upsert whose quote matches a stale anchor of an existing
  requirement in the same section lands on that requirement. See
  [mutations](../graph.md#mutations).
- A derived requirement keys on its `statement` within its `from` set. A decree
  requirement keys on `statement` alone. When a `reconcile-section` upsert's statement
  matches a derived or decree requirement whose ratification proposal targets that
  section, the upsert lands on that node and gives it a `source`. That is ratification
  completing by hand. See [ratification proposals](./diagnostic.md#ratification-proposals).
- The id is `req:<doc-stem>-<n>`, minted by the store, `req:x-<n>` for derived and decree
  requirements. E.g. `req:catalog-3`. See [identifiers](../model.md#identifiers).
- A change to `statement`, to the source `quote` in substance, or to `edges`,
  `transition`, or `facets` is a `requirement-revised` change record and opens
  [`rejudge-pair`](../goals/rejudge-pair.md) on the requirement's neighbors. Creation
  opens the same goal. See [goal derivation](../reconciler.md#goal-derivation).
- Deleting a requirement needs a `reason`. Its edges leave the relationships and its
  transition leaves the machine at commit. Curated views and instances that referenced it
  open [`retrace`](../goals/retrace.md).
