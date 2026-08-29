# Entity

An entity is a domain concept. Each entity carries one living `definition` that the
compiler refines as documents are reconciled. One concept, one node: there is never a
per-document copy of an entity.

## What is an entity

Entities are domain concepts: a system, a service, an interface, a type, a field, a
product, an actor, a character, a department. What kind of concept an entity is lives in
its free-form `stereotype`, so one node kind carries what would otherwise be a kind per
concept. Entities are not syntax artifacts:

- not file paths or directory names,
- not CLI flags or option names,
- not markdown constructs (a heading, a table, a link),
- not generic fragments ("the system", "the input"),
- not operation or function names (`createUser`, `addProduct`): what the system does is
  a requirement on the entity that does it, never an entity of its own,
- not technologies, languages, or third-party tools the system is built with (React,
  Go, PostgreSQL). Those belong in the requirement's statement text: "The gateway is
  built with Go" references the entity `gateway` only.

Entities exist because requirements need them. If no statement is about a concept, it is
not an entity. [`reconcile-section` sessions](../goals/reconcile-section.md) extract
requirements first and mint entities only as those requirements need them.

Granularity guidance: attach detail to a requirement before minting a sub-entity. "The
Shopping Cart shows a line-item count" is a requirement on `ent:shopping-cart`, not a new
`ent:line-item-count`. Mint the sub-entity only when statements are about it directly.
Structure the documents state ("the shop consists of the order service and the inventory
service") is [containment](#containment). Structure invented to tame scale comes from the
[`abstract-entity` goal](../goals/abstract-entity.md) and carries `derived` provenance
until the documents state it.

One sentence, one subject: a sentence that names its subject twice introduces one
entity, not two. "This software is a warehouse management system" defines a single
concept; "this software" and "warehouse management system" are the same node, the
second wording an alias.

Instances are entities too. "Ana, a gold-tier customer, keeps 3 items in her cart" mints
`ent:ana` with `tier: gold` on its `attributes`, tied to `ent:customer` by an
`instantiation` edge on the requirement that states it. Conformance stays checkable: the
[`conform-instance` goal](../goals/conform-instance.md) judges values and links against
the type, and the mechanical part (every attribute name on the instance exists on the
type or one of its generalizations) is the `nonconformant-instance` check.

The [validation gates](../graph.md#validation-gates) reject names that look like syntax
rather than a concept, unless the call carries an explaining `note`. They also reject a
name that is a variant of an existing entity's name ("backend" beside "backend
system"): one concept, one node; the wording joins the existing entity's `aliases`
instead. A `note` saying how the concepts differ overrides when the resemblance is
coincidental.

## Fields

- `name`: the primary handle. `aliases`: alternate names seen in the documents.
- `definition`: the one living definition, refined as documents are reconciled. It is
  never forked per document.
- `scope`: keeps distinct same-name concepts apart. See [scopes](../concepts/scopes.md).
- `stereotype`: a free-form judged label: `system`, `service`, `interface`, `actor`,
  `table`, `character`, `department`, whatever the model judges the medium calls the
  concept. Nothing enumerates the allowed values, and the harness keys on structure, not
  on the label: an entity is interface-like because something realizes it, a container
  because it has children. Two labels are conventions the renderer and the provider check
  recognize: `actor` draws as an actor, and `interface` counts as interface-like before
  anything realizes it. Every other label is drawn as text. The stereotype is a judgment,
  recorded with `reasoning` and backed by the entity's mentions like every other fact on
  the node.
- `parent`: the containing entity. One tree, unlimited depth. See
  [containment](#containment).
- `attributes`: `[{name, type?, value?, provenance}]`. A `type` where prose states
  structure ("an order carries a total and a currency"). A `value` where the entity is an
  instance ("priced in EUR"). Behavior is never an attribute; it is a requirement. Each
  attribute carries its own [provenance](../model.md#provenance). `name` is the
  attribute's key within the entity: an upsert naming an existing attribute refreshes it.
- `mentions`: `[{doc, section, quote}]`, the sections that talk about the entity. Each
  `quote` is verbatim. Mentions are the entity's quote provenance.
- `provenance`: `{derived: {from, reasoning}}` or `{decree: {author, at, note}}`,
  present on an entity no document states, whatever mentions its requirements add by
  reference. A mention that names the entity removes it: an `upsert_entity` whose
  `mention` names it, or an accepted
  [ratification proposal](./diagnostic.md#ratification-proposals). The entity is then
  quoted. A mention a committed requirement adds by reference does not remove it: that
  mention says the documents use the concept, not that they state it.
- `limits`: per-node bumps above the built-in limits. See
  [shared fields](../model.md#shared-fields).
- `confidence` and `reasoning`.
- `created` and `updated`: generation markers.

E.g.:

```yaml
ent:order:
  name: Order
  aliases: [customer order]
  definition: A confirmed purchase of one or more items by a customer.
  scope: commerce
  parent: ent:order-service
  attributes:
    - {name: total, provenance: {quote: {doc: docs/shop.md, section: /shop/orders,
        quote: "An order carries a total and a currency."}}}
    - {name: currency, provenance: {quote: {doc: docs/shop.md, section: /shop/orders,
        quote: "An order carries a total and a currency."}}}
  mentions:
    - {doc: docs/shop.md, section: /shop/orders, quote: "An order carries a total and a currency."}
    - {doc: docs/shop.md, section: /shop/checkout, quote: "When payment succeeds, the order becomes paid."}
  limits: {requirements-per-entity: 70}
  confidence: 0.9
  reasoning: The documents treat the order as the unit of purchase, distinct from the cart.
  created: g3
  updated: g12
```

An instance:

```yaml
ent:ana:
  name: Ana
  definition: A gold-tier customer used as the worked example.
  scope: commerce
  attributes:
    - {name: tier, value: gold, provenance: {quote: {doc: docs/shop.md, section: /shop/examples,
        quote: "Ana, a gold-tier customer, keeps 3 items in her cart, priced in EUR."}}}
  mentions:
    - {doc: docs/shop.md, section: /shop/examples,
       quote: "Ana, a gold-tier customer, keeps 3 items in her cart, priced in EUR."}
  created: g5
  updated: g5
```

## Containment

- One tree. `parent` names the containing entity; a root has none. A system contains
  services, a service contains modules, a module contains its domain concepts, a database
  its tables. Depth is unlimited. The gates reject a `parent` that does not exist and a
  `parent` that would close a cycle.
- Composition consistency. Where the documents state a whole-part, the statement yields a
  `composition` edge (the whole as `a`, the part as `b`) and the session sets `parent` on
  the part to match. A `composition` never crosses the tree sideways: for every
  `composition` contribution, the part's `parent` and the whole are comparable, meaning
  one contains the other or they are the same node. A part with no `parent` is always
  consistent. A part and a whole in separate branches draw `containment-mismatch`. E.g.
  `ent:order-item` (parent `ent:order-service`) composed into `ent:shopping-cart` (parent
  `ent:order-service`) is consistent; the same part under `ent:inventory-service` is a
  mismatch. See [checks](../compilation.md#checks).
- Limits. `children-per-entity` (10 soft, 20 hard) and `requirements-per-entity` (50
  soft, 80 hard) open the [`abstract-entity` goal](../goals/abstract-entity.md) on the
  entity: sub-entities with `parent`, detail moved down, docs proposals staged. The
  `states-per-state-machine` limit opens the same goal on the machine's subject. A per-node
  bump in `limits` raises the threshold. See [limits](../graph.md#limits).
- Lifting. Containment is what lets a coarse view stay true. When a view shows an ancestor
  and hides its descendants, every relationship touching a hidden descendant lifts to the
  nearest shown ancestor at render time. Lifting stores nothing. See
  [rendering](./relationship.md#rendering) and
  [lifting and collapse](../diagrams.md#lifting-and-collapse).
- Consumers read the tree. Generation groups by container where containment exists;
  package, component, and composite views draw it. See
  [generation](../../consumers/gen.md) and [view kinds](./view.md#kinds).

## Identity

- The natural key is `name` plus `scope`, and `parent` joins the key when the caller
  supplies it. `upsert_entity` keys on it, so a retried create lands on the existing node
  instead of duplicating it. An upsert without `parent` matches when exactly one entity
  with that name and scope exists, whatever its parent. Several matches is an error naming
  the candidates and asking for `parent`. A wrong merge is the failure to avoid. See
  [the natural key under containment](../concepts/identity.md#the-natural-key-under-containment)
  and [mutations](../graph.md#mutations).
- The id `ent:<slug>` is minted at creation and never changes. A rename keeps the id, so
  the slug can go stale. That is expected. See [identifiers](../model.md#identifiers).
- A merge keeps one entity and leaves a redirect from the absorbed id to the survivor, so
  downstream consumers holding the old id still resolve. The absorbed entity's mentions,
  attributes, children, and edges move to the survivor before derived data recomputes.
- A natural key deleted and recreated across builds is `unstable-extraction`. A key that a
  GC split and a compile merge flip back and forth is `unstable-derivation`: the pair
  parks, blocked on a human. See [flip detection](../reconciler.md#flip-detection).
