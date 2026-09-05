# split-view

Grades view splitting ([`split-view`](../../compiler/goals/split-view.md)) on a
sequence view over its participant limit. The goal derives from the seeded fixture:
the curated view draws ten participants against a soft limit of eight
(`participants-per-sequence-view`), so the seeding commit writes `threshold-crossed`
on it and the board derives the goal with its `break after` hint at the section
boundary ([derived goals](../cases.md#derived-goals)).

The document states the break: `## Checkout` (six steps among six participants) then
`## Fulfillment` (six steps among six participants, the customer and the order service
in both). The split follows that boundary: at least one sub-view of the same kind
exists, the original is within its limit, and every original member is still in the
original, in a sub-view, or excluded with a note. Dropping a step to lower the count
is the wrong call, and so is a split along a structure the document does not state.

```yaml
name: split-view
description: Split a sequence view over its participant limit along the section boundary the document states, dropping nothing.
tier: structure
par:
  rounds: 4
goal:
  kind: split-view
  target: view:sequence/purchase
given:
  docs:
    docs/purchase.md: |
      # Purchase flow

      The shop sells to customers through a storefront.

      ## Checkout

      The customer submits the cart to the storefront.
      The storefront asks the cart service for the cart contents.
      The storefront asks the pricing service for the total.
      The storefront charges the total through the payment gateway.
      The payment gateway confirms the charge to the storefront.
      The storefront places the order with the order service.

      ## Fulfillment

      The order service reserves the items at the warehouse.
      The warehouse packs the order and requests a pickup from the carrier.
      The carrier reports the tracking number to the order service.
      The order service records the sale in the ledger.
      The order service asks the notifier to email the customer.
      The notifier sends the shipping confirmation to the customer.
  graph:
    entities:
      ent:shop:
        name: Shop
        stereotype: system
        definition: The system that sells to customers.
        mentions:
          - section: 'docs/purchase.md#/purchase-flow'
            quote: The shop sells to customers through a storefront.
      ent:customer:
        name: Customer
        stereotype: actor
        definition: A person buying from the shop.
        mentions:
          - section: 'docs/purchase.md#/purchase-flow/checkout'
            quote: The customer submits the cart to the storefront.
      ent:storefront:
        name: Storefront
        stereotype: component
        parent: ent:shop
        definition: The shop's customer-facing front.
        mentions:
          - section: 'docs/purchase.md#/purchase-flow/checkout'
            quote: The customer submits the cart to the storefront.
      ent:cart-service:
        name: Cart Service
        stereotype: service
        parent: ent:shop
        definition: Holds cart contents.
        mentions:
          - section: 'docs/purchase.md#/purchase-flow/checkout'
            quote: The storefront asks the cart service for the cart contents.
      ent:pricing-service:
        name: Pricing Service
        stereotype: service
        parent: ent:shop
        definition: Computes totals.
        mentions:
          - section: 'docs/purchase.md#/purchase-flow/checkout'
            quote: The storefront asks the pricing service for the total.
      ent:payment-gateway:
        name: Payment Gateway
        stereotype: system
        definition: The outside system that charges cards.
        mentions:
          - section: 'docs/purchase.md#/purchase-flow/checkout'
            quote: The storefront charges the total through the payment gateway.
      ent:order-service:
        name: Order Service
        stereotype: service
        parent: ent:shop
        definition: Takes and fulfills orders.
        mentions:
          - section: 'docs/purchase.md#/purchase-flow/checkout'
            quote: The storefront places the order with the order service.
      ent:warehouse:
        name: Warehouse
        stereotype: component
        parent: ent:shop
        definition: Reserves and packs items.
        mentions:
          - section: 'docs/purchase.md#/purchase-flow/fulfillment'
            quote: The order service reserves the items at the warehouse.
      ent:carrier:
        name: Carrier
        stereotype: actor
        definition: The outside party that ships packages.
        mentions:
          - section: 'docs/purchase.md#/purchase-flow/fulfillment'
            quote: The warehouse packs the order and requests a pickup from the carrier.
      ent:ledger:
        name: Ledger
        stereotype: component
        parent: ent:shop
        definition: Records sales.
        mentions:
          - section: 'docs/purchase.md#/purchase-flow/fulfillment'
            quote: The order service records the sale in the ledger.
      ent:notifier:
        name: Notifier
        stereotype: component
        parent: ent:shop
        definition: Sends emails to customers.
        mentions:
          - section: 'docs/purchase.md#/purchase-flow/fulfillment'
            quote: The order service asks the notifier to email the customer.
    requirements:
      req:purchase-1:
        statement: The customer submits the cart to the storefront.
        entities: [ent:customer, ent:storefront]
        edges:
          - {a: ent:customer, b: ent:storefront, type: dependency}
        facets:
          - {facet: behavior, reasoning: a step of the checkout}
        source:
          section: 'docs/purchase.md#/purchase-flow/checkout'
          quote: The customer submits the cart to the storefront.
      req:purchase-2:
        statement: The storefront asks the cart service for the cart contents.
        entities: [ent:storefront, ent:cart-service]
        edges:
          - {a: ent:storefront, b: ent:cart-service, type: dependency}
        facets:
          - {facet: behavior, reasoning: a step of the checkout}
        source:
          section: 'docs/purchase.md#/purchase-flow/checkout'
          quote: The storefront asks the cart service for the cart contents.
      req:purchase-3:
        statement: The storefront asks the pricing service for the total.
        entities: [ent:storefront, ent:pricing-service]
        edges:
          - {a: ent:storefront, b: ent:pricing-service, type: dependency}
        facets:
          - {facet: behavior, reasoning: a step of the checkout}
        source:
          section: 'docs/purchase.md#/purchase-flow/checkout'
          quote: The storefront asks the pricing service for the total.
      req:purchase-4:
        statement: The storefront charges the total through the payment gateway.
        entities: [ent:storefront, ent:payment-gateway]
        edges:
          - {a: ent:storefront, b: ent:payment-gateway, type: dependency}
        facets:
          - {facet: behavior, reasoning: a step of the checkout}
        source:
          section: 'docs/purchase.md#/purchase-flow/checkout'
          quote: The storefront charges the total through the payment gateway.
      req:purchase-5:
        statement: The payment gateway confirms the charge to the storefront.
        entities: [ent:payment-gateway, ent:storefront]
        edges:
          - {a: ent:payment-gateway, b: ent:storefront, type: dependency}
        facets:
          - {facet: behavior, reasoning: a step of the checkout}
        source:
          section: 'docs/purchase.md#/purchase-flow/checkout'
          quote: The payment gateway confirms the charge to the storefront.
      req:purchase-6:
        statement: The storefront places the order with the order service.
        entities: [ent:storefront, ent:order-service]
        edges:
          - {a: ent:storefront, b: ent:order-service, type: dependency}
        facets:
          - {facet: behavior, reasoning: a step of the checkout}
        source:
          section: 'docs/purchase.md#/purchase-flow/checkout'
          quote: The storefront places the order with the order service.
      req:purchase-7:
        statement: The order service reserves the items at the warehouse.
        entities: [ent:order-service, ent:warehouse]
        edges:
          - {a: ent:order-service, b: ent:warehouse, type: dependency}
        facets:
          - {facet: behavior, reasoning: a step of the fulfillment}
        source:
          section: 'docs/purchase.md#/purchase-flow/fulfillment'
          quote: The order service reserves the items at the warehouse.
      req:purchase-8:
        statement: The warehouse packs the order and requests a pickup from the carrier.
        entities: [ent:warehouse, ent:carrier]
        edges:
          - {a: ent:warehouse, b: ent:carrier, type: dependency}
        facets:
          - {facet: behavior, reasoning: a step of the fulfillment}
        source:
          section: 'docs/purchase.md#/purchase-flow/fulfillment'
          quote: The warehouse packs the order and requests a pickup from the carrier.
      req:purchase-9:
        statement: The carrier reports the tracking number to the order service.
        entities: [ent:carrier, ent:order-service]
        edges:
          - {a: ent:carrier, b: ent:order-service, type: dependency}
        facets:
          - {facet: behavior, reasoning: a step of the fulfillment}
        source:
          section: 'docs/purchase.md#/purchase-flow/fulfillment'
          quote: The carrier reports the tracking number to the order service.
      req:purchase-10:
        statement: The order service records the sale in the ledger.
        entities: [ent:order-service, ent:ledger]
        edges:
          - {a: ent:order-service, b: ent:ledger, type: dependency}
        facets:
          - {facet: behavior, reasoning: a step of the fulfillment}
        source:
          section: 'docs/purchase.md#/purchase-flow/fulfillment'
          quote: The order service records the sale in the ledger.
      req:purchase-11:
        statement: The order service asks the notifier to email the customer.
        entities: [ent:order-service, ent:notifier]
        edges:
          - {a: ent:order-service, b: ent:notifier, type: dependency}
        facets:
          - {facet: behavior, reasoning: a step of the fulfillment}
        source:
          section: 'docs/purchase.md#/purchase-flow/fulfillment'
          quote: The order service asks the notifier to email the customer.
      req:purchase-12:
        statement: The notifier sends the shipping confirmation to the customer.
        entities: [ent:notifier, ent:customer]
        edges:
          - {a: ent:notifier, b: ent:customer, type: dependency}
        facets:
          - {facet: behavior, reasoning: a step of the fulfillment}
        source:
          section: 'docs/purchase.md#/purchase-flow/fulfillment'
          quote: The notifier sends the shipping confirmation to the customer.
    views:
      view:sequence/purchase:
        kind: sequence
        title: Purchase flow
        members:
          - req:purchase-1
          - req:purchase-2
          - req:purchase-3
          - req:purchase-4
          - req:purchase-5
          - req:purchase-6
          - req:purchase-7
          - req:purchase-8
          - req:purchase-9
          - req:purchase-10
          - req:purchase-11
          - req:purchase-12
        provenance:
          derived:
            from:
              - req:purchase-1
              - req:purchase-2
              - req:purchase-3
              - req:purchase-4
              - req:purchase-5
              - req:purchase-6
              - req:purchase-7
              - req:purchase-8
              - req:purchase-9
              - req:purchase-10
              - req:purchase-11
              - req:purchase-12
            reasoning: The two sections of the purchase flow read as one sequence from cart to shipping confirmation.
assert:
  - nodeExists:
      id: view:sequence/purchase
  - viewWithinLimit:
      view: view:sequence/purchase
      limit: participants-per-sequence-view
  - viewExists:
      kind: sequence
      excluding: view:sequence/purchase
  - viewExists:
      kind: sequence
      titlePattern: 'checkout|fulfil'
      excluding: view:sequence/purchase
  - membersAccounted:
      view: view:sequence/purchase
      members:
        - req:purchase-1
        - req:purchase-2
        - req:purchase-3
        - req:purchase-4
        - req:purchase-5
        - req:purchase-6
        - req:purchase-7
        - req:purchase-8
        - req:purchase-9
        - req:purchase-10
        - req:purchase-11
        - req:purchase-12
```
