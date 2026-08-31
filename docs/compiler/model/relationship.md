# Relationship

A relationship is a derived node: the typed connections between two [entities](./entity.md),
one node per unordered pair. It is never written directly, and there is no write tool for
it. On every commit the store recomputes relationships from [requirement](./requirement.md)
`edges`. See [derived data](../graph.md#derived-data).

An edge with no requirement cannot exist. Every relationship carries provenance for free:
each contribution names exactly the statements that tie the pair together in that
direction and type.

## Fields

- The map key is the id: `rel:<a>~<b>`, the two member entity slugs in lexical order. E.g.
  `rel:order-item~shopping-cart`. Like every node kind, the id is not repeated inside the
  record.
- `members: [a, b]`: the two entity ids, in the id's order.
- `contributions: [{a, b, type, cardinality?, requirements: [ids]}]`: one group per
  direction and type. `a` acts on `b`. `requirements` lists the contributing requirement
  ids and is never empty. A pair can carry `a→b dependency`, `a→b association`, and
  `b→a dependency` side by side, each group with its own requirements.
- `cardinality` on a group: present when every contributing edge that states one states
  the same one. Disagreeing cardinalities leave the group without one. Each requirement's
  own edge keeps its value, and [`rejudge-pair`](../goals/rejudge-pair.md) sees the
  disagreement when either requirement changes.

E.g.:

```yaml
rel:order-item~shopping-cart:
  members: [ent:order-item, ent:shopping-cart]
  contributions:
    - {a: ent:shopping-cart, b: ent:order-item, type: composition, cardinality: "1..*",
       requirements: [req:shop-6]}
```

## Types

The UML set, seven types. Six of them rank, strongest first, and the rank decides what a
collapsed arrow shows:

generalization → realization → composition → aggregation → association → dependency

- `generalization`: is-a (a Dog is an Animal). `a` is the specific, `b` the general.
- `realization`: fulfills a contract without inheriting implementation (an ArrayList
  realizes a List). `a` is the realizer, `b` the contract. Provides is a `realization`
  toward an interface entity.
- `composition`: an owned part (a House is composed of Rooms). `a` is the whole, `b` the
  part. Containment follows it. See [containment](./entity.md#containment).
- `aggregation`: a shared part, independent of the whole (a Driver in a Car). `a` is the
  whole.
- `association`: a lasting connection, one holds a reference to the other (a Student and
  a Course).
- `dependency`: temporary use (a CreditCard depends on FraudDetection). Requires is a
  `dependency` toward an interface entity. An edge with no declared `type` contributes
  `dependency`, the weakest structural claim.

The seventh, `instantiation`, stands outside the ranking: is-an-instance-of (Ana is a
customer). `a` is the instance, `b` the type. The instance's values live on its
`attributes`. An `instantiation` group never promotes with the others: a pair that carries
one keeps it as a separate group, whatever else the pair carries. It is never drawn as an
arrow: the object view reads it to name the instance's type (`ana : Customer`).

The rank matters where arrows collapse: a lifted or collapsed arrow shows the strongest
ranked type among the groups beneath it.

## Recompute

On every commit, after the changeset lands:

- Every entry in every requirement's `edges` contributes one edge `{a, b, type,
  cardinality?}`, an untyped edge as `dependency`.
- Edges group by unordered pair into one relationship node, then by `(a, b, type)` into
  contributions. Contributions and their `requirements` are ordered by id, so the shard
  diffs cleanly.
- A group whose last contributing edge disappears (requirement deleted, edges changed,
  entity merged) is removed. A relationship with no group is removed. A merge redirects
  the absorbed entity's edges to the survivor before the recompute. No cleanup goal runs
  for it, and nothing retraces derived data.
- The recompute is per pair and per direction: an edge change touches only its own pair's
  node.
- The `related` axis of the loaded set walks relationships, `instantiation` included. See
  [axes](../context.md#axes).
- Consumers read the groups. Generation orders entities topologically over the
  contributions, each type with its own direction rule. Project management maps
  `composition` and `aggregation` to parent-child and `dependency` and `realization` to
  blocking links. See [generation](../../consumers/gen.md#order-from-relationships) and
  [project management](../../consumers/pm.md#graph-to-tracker-items).

## Rendering

Rendering happens in the emitters, never in the store. A view renders its members plus
every relationship among them:

- One arrow per ranked contribution group, drawn from `a` to `b` in the type's UML
  notation, labeled with the cardinality when the group carries one. An `instantiation`
  group draws no arrow; the object view names the instance's type from it.
- Lifting: when a view hides a descendant, a relationship touching it lifts to the nearest
  shown ancestor. Groups that lift onto the same shown pair and direction collapse to one
  arrow, drawn in the strongest ranked type among them with an ASCII count label
  (`: 3 edges`). An `instantiation` group never joins a collapse: it keeps its own
  group and draws no arrow.
- Collapse: a member listed in the view's `collapse` hides its subtree. The same
  aggregation applies.
- A lifted arrow's justification is the set of concrete contributions beneath it. Clicking
  it in the GUI lists them, each walking to its requirement and quote. Lifting stores
  nothing, so it cannot drift.
- Arrows count toward `edges-per-view` (40 soft, 60 hard, resolved by
  [`split-view`](../goals/split-view.md)). A view never silently omits an arrow.

See [lifting and collapse](../diagrams.md#lifting-and-collapse) and
[the emitters](../diagrams.md#the-emitters).
