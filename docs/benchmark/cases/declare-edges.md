# declare-edges

Grades edge declaration on a settled statement
([`declare-edges`](../../compiler/goals/declare-edges.md)), in two cases. The goal
derives from the seeded fixture: the seeding commit writes `edges-missing` on a
requirement that lists two or more entities and no edges
([derived goals](../cases.md#derived-goals)).

## A whole-part sentence

"The order service consists of the cart module and the pricing module" ties the whole
to each part: two `composition` edges from the order service, none between the
modules, the statement and its entities untouched. The derived relationships follow.

```yaml
name: declare-edges
description: A whole-part sentence yields a composition edge from the whole to each part and none between the parts.
tier: structure
par:
  rounds: 2
goal:
  kind: declare-edges
  target: req:orders-1
given:
  docs:
    docs/orders.md: |
      # Orders

      ## Structure

      The order service consists of the cart module and the pricing module.
  graph:
    entities:
      ent:order-service:
        name: Order Service
        stereotype: service
        definition: The service that takes orders.
        mentions:
          - section: 'docs/orders.md#/orders/structure'
            quote: The order service consists of the cart module and the pricing module.
      ent:cart-module:
        name: Cart Module
        stereotype: module
        definition: The module that holds a customer's cart.
        mentions:
          - section: 'docs/orders.md#/orders/structure'
            quote: the cart module
      ent:pricing-module:
        name: Pricing Module
        stereotype: module
        definition: The module that computes totals.
        mentions:
          - section: 'docs/orders.md#/orders/structure'
            quote: the pricing module
    requirements:
      req:orders-1:
        statement: The order service consists of the cart module and the pricing module.
        entities: [ent:order-service, ent:cart-module, ent:pricing-module]
        source:
          section: 'docs/orders.md#/orders/structure'
          quote: The order service consists of the cart module and the pricing module.
assert:
  - edgeDeclared:
      requirement: req:orders-1
      a: ent:order-service
      b: ent:cart-module
      type: composition
  - edgeDeclared:
      requirement: req:orders-1
      a: ent:order-service
      b: ent:pricing-module
      type: composition
  - edgeAbsent:
      requirement: req:orders-1
      a: ent:cart-module
      b: ent:pricing-module
  - relationshipExists:
      a: Order Service
      b: Cart Module
      type: composition
  - requirementExists:
      statementPattern: 'consists of the cart module'
      entity: ent:order-service
```

## Not structural

"The order total is shown in the customer's currency" names the order and the customer
and relates neither to the other: the customer is context. The session declares
nothing and marks the goal done as `not-structural`. Staging any edge, or a
`dependency` guessed to satisfy the goal, is the wrong call.

```yaml
name: declare-edges-none
description: A sentence whose second entity is context yields no edge and no mutation.
tier: structure
par:
  rounds: 2
goal:
  kind: declare-edges
  target: req:orders-1
given:
  docs:
    docs/orders.md: |
      # Orders

      ## Display

      The order total is shown in the customer's currency.
  graph:
    entities:
      ent:order:
        name: Order
        definition: A customer's purchase.
        mentions:
          - section: 'docs/orders.md#/orders/display'
            quote: The order total is shown in the customer's currency.
      ent:customer:
        name: Customer
        stereotype: actor
        definition: A person buying from the shop.
        mentions:
          - section: 'docs/orders.md#/orders/display'
            quote: the customer's currency
    requirements:
      req:orders-1:
        statement: The order total is shown in the customer's currency.
        entities: [ent:order, ent:customer]
        source:
          section: 'docs/orders.md#/orders/display'
          quote: The order total is shown in the customer's currency.
assert:
  - mutationCount:
      max: 0
  - edgeAbsent:
      requirement: req:orders-1
      a: ent:order
      b: ent:customer
  - nodeExists:
      id: req:orders-1
```
