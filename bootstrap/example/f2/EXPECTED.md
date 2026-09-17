# F2 expected outcomes (hand-labeled, not part of the docs glob)

Counts are indicative, not graded (owner ruling 2026-08-14): a richer graph is
acceptable when every extra requirement is independently testable and every extra
entity has statements directly about it. The graded part is the traps.

## Entities

Core entities (must exist), with the stereotype and parent the prose supports:

- Orderly: `system`, the containment root ("Orderly is an online shop system").
- Customer: `actor`. No parent, actors stand outside the system. Attribute `email`
  ("Each Customer shall have a unique Email address").
- Operator (borderline): `actor`, the subject of admin.md's commands and of "Operators
  manage the system with the Admin CLI". Acceptable to leave unminted (see flows).
- Admin CLI: a stereotype naming the tool (`cli` or `tool`), never `actor` ("the
  operator's tool for running Orderly"); no stereotype is a weaker result, not a
  failure. Parent Orderly is acceptable (`orderly serve` starts the system), and so is
  no parent: the front door states no containment, and a document that only says what
  the system's parts cover leaves the top level flat
  ([enumerations](../../../docs/compiler/concepts/statements.md#enumerations)).
- Inventory, Catalog: parent Orderly or no parent, on the same rule ("The warehouse side
  is covered by Inventory" states coverage, not containment). A parent outside Orderly
  is wrong.
- Order, Payment, Shipment, Return, Product, Stock (or Inventory's `Stock`): domain
  concepts. Parent Orderly or one of its parts is acceptable, no parent is acceptable, a
  parent outside Orderly is wrong. Product carries `name`, `price` ("A Product has a name
  and a price") and `category` ("exactly one Catalog category"). Order carries
  `quantity` and `price at placement` from "list each Product with its quantity and the
  price at the time of placement", or keeps them as statement detail.
- Email (borderline): either an attribute on Customer or an entity Customer composes.

`buyer` must NOT survive as a separate entity (trap 2). `unused-entity` must not fire on
any core entity. No entity has a stereotype of `interface`: nothing is realized here.

## Requirements and edges

At least 30 requirements across the docs, one per obligation, free-form statements.
Declarative sentences count ("A Payment settles an Order", "A Product has a name and a
price"); "shall" is neither required nor a signal. No obligation the docs state may be
missing. Edges the prose states, `a` acting on `b`:

- Customer → Order, `association` ("a purchase a Customer places").
- Order → Product, `aggregation`, cardinality `1..*` ("contains one or more Products").
  Products are shared across Orders, so `composition` is the wrong type; a `composition`
  here with Product under Catalog also draws `containment-mismatch`.
- Catalog → Product, `aggregation` ("lists every Product Orderly sells").
- Payment → Order, `association` or `dependency` ("A Payment settles an Order").
- Shipment → Order, `association`; Shipment → Customer, `dependency` ("delivers a packed
  Order to its buyer").
- Return → Customer and Return → Inventory, `dependency` ("brings goods from a Customer
  back into Inventory"); Return → Order, `association` ("a Return for an Order").
- Inventory → Stock, `aggregation` or `association`; Stock → Product, `association`
  ("tracks the Stock of every Product").
- Order → Stock, `dependency` ("reserve the Stock for each Product in it").
- Operator → Admin CLI, `dependency`; Admin CLI → Orderly, `dependency` ("the operator's
  tool for running Orderly").
- Orderly → Inventory, Orderly → Admin CLI, Orderly → Catalog, `composition`, matching
  the parents above. Composition consistency must hold: no `containment-mismatch`.
- Customer → Orderly, `association` ("holds an account with Orderly"); Customer →
  Catalog, `dependency` ("browses the Catalog").

Multi-entity requirements left without edges open optional `declare-edges` goals; they
count as `optional advised`, never as open mandatory work.

system.md's delivery sentence ("Orderly shall be delivered as a Rust crate verified by
`cargo test`") yields exactly one requirement on Orderly, no edge. `Rust`, `crate`, and
`cargo test` name the medium and the tool that verifies it, never an entity.

## Generation medium

The delivery sentence is the only statement of what the deliverable is made of. The
[medium decision](../../../docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)
reads it (and the planted `product/` tree, which pins the same answer): the ledger's
`medium` records a Rust crate with `cargo` as the toolchain, and every test row runs
`cargo test`. A ledger naming Python, argparse, or pytest is a failure: the medium was
guessed from admin.md's command-line flavor instead of read from the statement.

## The Order lifecycle

orders.md and payment.md describe state changes of the Order. The derived machine
`sm:order`:

- States: `placed`, `paid`, `on hold`, `cancelled`. Initial: `placed` (the only state
  no transition enters; "placement" names it).
- Transitions the prose states:
  - `placed → paid`, trigger `Payment confirmed` (payment.md).
  - `placed → on hold`, trigger `Payment fails`, guard `three times` (payment.md).
  - `placed → cancelled`, trigger `not paid`, guard `21 days after placement`
    (orders.md).
- Optional, acceptable when extracted: `placed → cancelled` with guard `30 days after
  placement` from payment.md's deadline sentence, a second arrow on the same pair (trap
  3). Order states read from shipping.md ("packed", "delivery": `paid → shipped →
  delivered`) extend the machine; not required.
- Checks on `sm:order`:
  - `dead-end-state` (info), one diagnostic naming `paid`, `on hold`, `cancelled` (fewer
    when shipping extends `paid`). `cancelled` is the final state, acknowledged by a
    human. `on hold` is a real gap: no document says how an Order leaves hold.
  - `unhandled-event` (info), one diagnostic: every trigger is handled only in `placed`;
    `(on hold, Payment confirmed)` is a gap the docs leave.
  - `unreachable-state` stays silent (everything is reachable from `placed`).
  - `nondeterministic-transition` stays silent even with both `placed → cancelled`
    arrows: the guards differ and the check takes distinct guards as disjoint. The
    contradiction is trap 3's `contradiction` diagnostic, not a machine check.
  - Under `states-per-state-machine`; no `abstract-entity` goal on Order.

No other entity has a machine: shipping.md and returns.md name events on Shipment and
Return ("leaves the warehouse", "received and inspected") but no state pair for them. An
optional `sm:product` (`in stock → hidden`), grounded in catalog.md's explicit
shown/hidden pair, is tolerated; Shipment and Return machines stay wrong.

## Instances

None. No document holds a worked example: inventory.md's "40 units of SKU-1042 arrive in
the morning" is illustration under a section that states one rule (trap 4), not an
instance of Product. No `instantiation` edge, no `conform-instance` goal, no
`view:object/*`.

## Flow clusters and default views

Behavior-facet requirements cluster by document, the author's own cut of the subject
into flows ([flow clusters](../../../docs/compiler/model/view.md#level-views)); the
actors are read off the members when the view renders. Clusters of two or more derive a
`use-case` and a `sequence` view sharing a title. A sequence view derives only when at
least two members carry an edge; a use-case view without a sequence twin is
mechanism-faithful for an edge-poor cluster. Expected:

| cluster | members (document order) | views |
| --- | --- | --- |
| shipping.md (actors: Customer) | delivers a packed Order to its buyer; tracking link on dispatch; two failed attempts, return and refund (`failure-mode`) | `view:usecase/shipping`, `view:sequence/shipping`, title `Shipping` |
| admin.md (actors: Operator) | `orderly serve` starts the system; `orderly report` for the daily numbers; every Order from the selected period | `view:usecase/admin`, `view:sequence/admin`, title `Admin CLI` |
| returns.md (actors: Customer) | brings goods back into Inventory; open a Return within 30 days of delivery | `view:usecase/returns`, `view:sequence/returns`, title `Returns` |
| orders.md (actors: Customer) | a Customer places an Order; submit, reserve Stock | `view:usecase/orders`, `view:sequence/orders`, title `Order` |

When Operator is not minted, the admin.md cluster still derives (`admin`, title
`Admin CLI`) and the use case draws no actor, never the Admin CLI as a stick figure.
The other documents cluster the same way: system.md, customer.md, payment.md,
inventory.md, and catalog.md each derive a use case titled by the document when two or
more of their behavior statements reach the root level (`Orderly`, `Customer`,
`Payment`, `Inventory`, `Catalog`), each drawing the actors its members name (Customer
where the statements say so, none for the stock rules): mechanism-faithful surplus, and
the four named clusters above are the graded minimum. A use case never draws a
component as an actor (`actor Shipment` beside an inventory flow is a failure). Each
lone member draws `unplaced-behavior` (info) with a `flow-unplaced` record and an
optional `curate-view` goal: `optional advised` in the verdict, never blocking
convergence.

Structural defaults are level views: one per node with two or more children, the scope
root included (docs/compiler/model/view.md#level-views). The kind is `component` when
the node or any child carries a structural stereotype (`system`, `component`,
`service`, `interface`, `actor`), `class` otherwise. Members are the direct children
plus every outside entity with a lifted edge into the level, in document order; no
`query`. Expected:

- `view:component/public`, title `Public`: the scope root's level view, the per-scope
  view under its old id. Members: the parentless entities (Orderly, Customer, Operator
  when minted, and every domain concept left without a parent), never "every entity":
  a child of Orderly appears in Orderly's level, not the root's. The kind is
  `component` because Orderly «system» and Customer «actor» are parentless; a compile
  that records neither stereotype derives `view:class/public` with the same members.
- `view:component/orderly`, title `Orderly`: derives only when Orderly has two or more
  children (Admin CLI, Inventory, Catalog per the entities above). Members: the
  children plus the outside entities whose edges lift into the level: Customer
  (browses the Catalog), Operator (uses the Admin CLI), and any parentless domain
  concept with an edge into a child (Order → Stock lifts to Inventory when Stock sits
  under Inventory; Return → Inventory). A compile that lands no containment has no
  Orderly level view; that is acceptable but weaker.
- A level view for any other node that gained two or more children (Catalog over
  Product and Catalog category: `view:class/catalog`; Inventory over Stock and a
  warehouse concept: `view:class/inventory`) is mechanism-faithful, not a defect. A
  node with one child has no level view.
- `view:state/order`, title `Order`, over `sm:order`.
- No `view:object/*`, no package, activity, deployment, or timing view: nothing curates
  them and no instance or time measure exists.

Flow views derive per level too. The root form keeps the unprefixed ids in the table
above (lifting to a parentless entity is identity). A deeper level clusters the
behavior requirements whose entities lift into its members, by document, under
`view:usecase/<node-slug>-<doc-slug>` and `view:sequence/<node-slug>-<doc-slug>`:
`view:usecase/orderly-admin`, titled `Orderly: Admin CLI`, when Admin CLI sits under
Orderly. These are mechanism-faithful
surplus; the four root clusters above are the graded minimum. Every rendered member
with a level view of its own carries a drill-down link (`[[../<kind>/<slug>.svg]]` in
the `.puml`, an anchor in the `.svg`): Orderly in `component/public` links to
`../component/orderly.svg`.

Every default carries `default: true` and derived provenance. A no-op rebuild derives
zero goals, makes zero LLM calls, and leaves every view and diagram byte-identical.

## Rendered diagrams

A compile must produce, each `.puml` with its `.svg` beside it and no `.png`:

```
jazyk-out/diagrams/component/public.puml        .svg   (class/public.puml when no parentless entity carries a structural stereotype)
jazyk-out/diagrams/component/orderly.puml       .svg   (when Orderly has two or more children)
jazyk-out/diagrams/usecase/shipping.puml            .svg
jazyk-out/diagrams/sequence/shipping.puml           .svg
jazyk-out/diagrams/usecase/admin.puml               .svg
jazyk-out/diagrams/sequence/admin.puml              .svg
jazyk-out/diagrams/usecase/returns.puml             .svg
jazyk-out/diagrams/sequence/returns.puml            .svg
jazyk-out/diagrams/state/order.puml             .svg
```

No `diagrams/object/` directory. Deeper level views (`usecase/orderly-*`,
`sequence/orderly-*`, `class/catalog`) add files under the same layout when their
levels exist. What the pictures show:

- `component/public`: `actor Customer`, `component Orderly` with its stereotype, each
  parentless entity once (exactly one `Customer`, one `Order` when Order is
  parentless); no element for a flag, path, or command. Arrows among the parentless
  entities, lifted from their descendants: `Customer -- Orderly` (the account, plus
  the browse of the Catalog lifted to Orderly), `Operator ..> Orderly` (the Admin CLI
  lifted), and, for the domain concepts left parentless, their stated types
  (`Order o-- "1..*" Product`, `Payment -- Order`, `Shipment -- Order`,
  `Shipment ..> Customer`). Orderly's element carries the drill-down link
  `[[../../docsgen/entities/orderly.md]]`, the entity's card, which holds the link to its level view. When the root view
  derives as `class/public`, the same members draw as classes with their attributes
  (`class Customer <<actor>>` with `email`, `class Product` with `name`, `price`,
  `category`).
- `component/orderly`: a `component` per child that landed (Catalog, Inventory,
  `"Admin CLI"`) plus the outside entities whose edges lift into the level
  (`actor Customer`, `actor Operator`, a parentless Order or Return). No interface, so
  no lollipop or socket. Arrows: `Customer ..> Catalog`, `Operator ..> "Admin CLI"`,
  `Order ..> Inventory` (Stock lifted) when those edges exist; a child with a level of
  its own (Catalog over two or more) carries its link.
- `usecase/shipping`: `actor Customer`, `usecase "Shipping"`,
  `Customer -- "Shipping"`. Never an actor named Buyer.
- `sequence/shipping`: `actor Customer`; one message per member that carries
  an edge, e.g. `Shipment -> Customer : ... tracking link (req:shipping-N)`; a member
  without an edge is a self-message on its first entity.
- `state/order`: `[*] --> placed`, `placed --> paid : Payment confirmed`,
  `placed --> on hold : Payment fails [three times]`,
  `placed --> cancelled : not paid [21 days after placement]`, and, when payment.md's
  deadline is extracted as a transition, a second `placed --> cancelled` arrow with
  `[30 days after placement]`: the contradiction, visible on the picture.

## Planted traps

1. Cross-doc identity: `Order` is defined in orders.md and used in payment.md,
   shipping.md, returns.md, system.md, admin.md. Must be ONE entity with mentions in at
   least 3 documents, hence one `sm:order` and one `view:state/order`.
2. Duplicate pair: shipping.md consistently says "buyer" for what customer.md calls
   Customer. Expect either reuse of `ent:customer` at extraction time (`buyer` joins its
   aliases), or a `review-entity` or `dedupe-candidates` merge, or a `duplicate-entity`
   diagnostic. A surviving `buyer` entity keys the shipping.md cluster as
   `buyer-shipping` and draws an actor named Buyer beside Customer: a visible failure.
3. Contradiction: orders.md says an Order is paid within 21 days of placement,
   payment.md says within 30 days. Expect exactly one `contradiction` diagnostic on
   `ent:order`, filed by `rejudge-pair`. Extracted as transitions, the two deadlines are
   two guards on one `placed → cancelled` pair, drawn as two arrows on `state/order`;
   `nondeterministic-transition` does not fire on them.
4. Non-normative trap: inventory.md "Examples" section hides a real rule ("the Stock
   count shall never go below zero", a `constraint` on Stock). Marking it non-normative
   must trigger `suspicious-non-normative`; extracting the rule and marking covered is
   also correct. The 40 units of SKU-1042 stay illustration: no entity, no instance.
5. Junk bait: admin.md is full of flags (`--port`, `--verbose`), paths
   (`/etc/orderly/config.toml`), and commands (`orderly serve`, `orderly report`). None
   of these may become entities; the commands and flags are requirements on the Admin
   CLI, with the identifiers kept verbatim in the statements. `Admin CLI` itself is a
   legitimate entity.
6. Genuinely non-normative: glossary.md and roadmap.md state no requirements and should
   be marked non-normative without a `suspicious-non-normative` finding. Nothing on
   either page mints an entity (no `Gift wrapping`, no `SKU`).
