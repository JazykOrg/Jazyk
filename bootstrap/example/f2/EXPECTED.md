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
- Admin CLI: `cli` or `tool`, never `actor` ("the operator's tool for running Orderly").
  Parent Orderly: `orderly serve` starts the system, so the tool is part of it.
- Inventory: parent Orderly ("The warehouse side is covered by Inventory").
- Catalog: parent Orderly (the Catalog is what Orderly sells through).
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
Return ("leaves the warehouse", "received and inspected") but no state pair for them.

## Instances

None. No document holds a worked example: inventory.md's "40 units of SKU-1042 arrive in
the morning" is illustration under a section that states one rule (trap 4), not an
instance of Product. No `instantiation` edge, no `conform-instance` goal, no
`view:object/*`.

## Flow clusters and default views

Behavior-facet requirements cluster by actor (the first `actor` among the requirement's
entities, else its first entity) and document. Clusters of two or more derive a
`use-case` and a `sequence` view sharing a title. A sequence view derives only when at
least two members carry an edge; a use-case view without a sequence twin is
mechanism-faithful for an edge-poor cluster. Expected:

| cluster | members (document order) | views |
| --- | --- | --- |
| Customer, shipping.md | delivers a packed Order to its buyer; tracking link on dispatch; two failed attempts, return and refund (`failure-mode`) | `view:usecase/customer-shipping`, `view:sequence/customer-shipping`, title `Customer: Shipping` |
| Operator, admin.md | `orderly serve` starts the system; `orderly report` for the daily numbers; every Order from the selected period | `view:usecase/operator-admin`, `view:sequence/operator-admin`, title `Operator: Admin CLI` |
| Customer, returns.md | brings goods back into Inventory; open a Return within 30 days of delivery | `view:usecase/customer-returns`, `view:sequence/customer-returns`, title `Customer: Returns` |
| Customer, orders.md (likely) | a Customer places an Order; submit, reserve Stock | `view:usecase/customer-orders`, `view:sequence/customer-orders`, title `Customer: Order` |

When Operator is not minted, the admin.md cluster keys on the Admin CLI instead
(`admin-cli-admin`, title `Admin CLI: Admin CLI`) and the use case draws the initiators
as actors. The cluster slug follows the actor's recorded name, so a plural `Operators`
keys `operators-admin`: acceptable. The remaining behavior requirements fall in one-member clusters (the
Customer in system.md, customer.md, payment.md; Payment, Catalog, Return in their own
pages; Inventory's two stock rules derive `Inventory: Inventory` only when both anchor
on Inventory first). Each lone member draws `unplaced-behavior` (info) with a
`flow-unplaced` record and an optional `curate-view` goal: `optional advised` in the
verdict, never blocking convergence.

Structural and state defaults:

- `view:class/public`, title `Public`, `query: {scope: public}`: every entity above.
- `view:component/orderly`, title `Orderly`: derives only when Orderly has at least one
  child. Members: Orderly's children plus the outside entities that depend on them
  (Customer, Operator). A compile that lands no containment has no component view; that
  is acceptable but weaker. A component view derives per containment root with at least
  one child (graph.md reads "system" structurally), so a `view:component/<root>` for
  another root that gained a child (Catalog over Catalog category) is mechanism-faithful,
  not a defect.
- `view:state/order`, title `Order`, over `sm:order`.
- No `view:object/*`, no package, activity, deployment, or timing view: nothing curates
  them and no instance or time measure exists.

Every default carries `default: true` and derived provenance. A no-op rebuild derives
zero goals, makes zero LLM calls, and leaves every view and diagram byte-identical.

## Rendered diagrams

A compile must produce, each `.puml` with its `.svg` beside it and no `.png`:

```
jazyk-out/diagrams/class/public.puml            .svg
jazyk-out/diagrams/component/orderly.puml       .svg   (when Orderly has children)
jazyk-out/diagrams/usecase/customer-shipping.puml   .svg
jazyk-out/diagrams/sequence/customer-shipping.puml  .svg
jazyk-out/diagrams/usecase/operator-admin.puml      .svg
jazyk-out/diagrams/sequence/operator-admin.puml     .svg
jazyk-out/diagrams/usecase/customer-returns.puml    .svg
jazyk-out/diagrams/sequence/customer-returns.puml   .svg
jazyk-out/diagrams/state/order.puml             .svg
```

No `diagrams/object/` directory. What the pictures show:

- `class/public`: `class Customer <<actor>>` with `email`; `class Product` with `name`,
  `price`, `category`; `class Orderly <<system>>`; `class "Admin CLI"` with its
  stereotype; exactly one `Customer` and one `Order` class; no class for a flag, path,
  or command. Arrows: `Customer -- Order`, `Order o-- "1..*" Product`,
  `Catalog o-- Product`, `Payment -- Order`, `Shipment -- Order`,
  `Shipment ..> Customer`, `Return ..> Inventory`, `Inventory -- Stock`,
  `Operator ..> "Admin CLI"`, `"Admin CLI" ..> Orderly`, `Orderly *-- Inventory`.
- `component/orderly`: `component Catalog`, `component Inventory`,
  `component "Admin CLI"`, `actor Customer`, `actor Operator`, `Customer ..> Catalog`,
  `Operator ..> "Admin CLI"`. No interface, so no lollipop or socket.
- `usecase/customer-shipping`: `actor Customer`, `usecase "Customer: Shipping"`,
  `Customer -- "Customer: Shipping"`. Never an actor named Buyer.
- `sequence/customer-shipping`: `actor Customer`; one message per member that carries
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
