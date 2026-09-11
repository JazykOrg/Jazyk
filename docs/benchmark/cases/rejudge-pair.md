# rejudge-pair

Grades pair judgment ([`rejudge-pair`](../../compiler/goals/rejudge-pair.md)), in two
cases. The goal derives from the seeded fixture: the seeding commit writes
`requirement-created` on both statements, the reconciler pairs them through their
shared entity and content tokens, and the board derives the pair goal
([derived goals](../cases.md#derived-goals)). The gate checks that a verdict with a
carrier exists; only the planted ground truth grades which verdict.

## A planted contradiction

Two documents hold a reserved item for different durations. The two statements
cannot both hold: the session files `contradiction` naming both requirements and
keeps both. A duplicate verdict, or a delete, is the wrong call.

```yaml
name: rejudge-pair-contradiction
description: Two statements that cannot both hold are filed as a contradiction naming both, and both are kept.
tier: review
par:
  rounds: 2
goal:
  kind: rejudge-pair
  target: req:inventory-1~req:orders-1
given:
  docs:
    docs/orders.md: |
      # Orders

      ## Reservations

      The order service shall hold a reserved item for 15 minutes before releasing it.
    docs/inventory.md: |
      # Inventory

      ## Reservations

      The order service shall hold a reserved item for 30 minutes before releasing it.
  graph:
    entities:
      ent:order-service:
        name: Order Service
        stereotype: service
        definition: The service that takes and holds orders.
        mentions:
          - section: 'docs/orders.md#/orders/reservations'
            quote: The order service shall hold a reserved item for 15 minutes before releasing it.
          - section: 'docs/inventory.md#/inventory/reservations'
            quote: The order service shall hold a reserved item for 30 minutes before releasing it.
    requirements:
      req:orders-1:
        statement: The order service shall hold a reserved item for 15 minutes before releasing it.
        entities: [ent:order-service]
        source:
          section: 'docs/orders.md#/orders/reservations'
          quote: The order service shall hold a reserved item for 15 minutes before releasing it.
      req:inventory-1:
        statement: The order service shall hold a reserved item for 30 minutes before releasing it.
        entities: [ent:order-service]
        source:
          section: 'docs/inventory.md#/inventory/reservations'
          quote: The order service shall hold a reserved item for 30 minutes before releasing it.
assert:
  - diagnosticExists:
      rule: contradiction
      subjects: [req:inventory-1, req:orders-1]
  - nodeExists:
      id: req:orders-1
  - nodeExists:
      id: req:inventory-1
  - diagnosticAbsent:
      rule: duplicate-requirement
```

## A planted duplicate across documents

Two documents state one obligation in two wordings. Different documents keep both:
the session files `duplicate-requirement` naming both and deletes nothing. A
contradiction verdict, or a delete of the worse-sourced side (the same-document rule),
is the wrong call.

```yaml
name: rejudge-pair-duplicate
description: One obligation stated in two documents is filed as a duplicate naming both, with both kept.
tier: review
par:
  rounds: 2
goal:
  kind: rejudge-pair
  target: req:orders-1~req:payment-1
given:
  docs:
    docs/orders.md: |
      # Orders

      ## Confirmation

      The checkout shall send the customer a confirmation email after the payment succeeds.
    docs/payment.md: |
      # Payment

      ## After payment

      After the payment succeeds, the checkout shall send the customer a confirmation email.
  graph:
    entities:
      ent:checkout:
        name: Checkout
        stereotype: component
        definition: The component that turns a cart into a paid order.
        mentions:
          - section: 'docs/orders.md#/orders/confirmation'
            quote: The checkout shall send the customer a confirmation email after the payment succeeds.
          - section: 'docs/payment.md#/payment/after-payment'
            quote: After the payment succeeds, the checkout shall send the customer a confirmation email.
    requirements:
      req:orders-1:
        statement: The checkout shall send the customer a confirmation email after the payment succeeds.
        entities: [ent:checkout]
        source:
          section: 'docs/orders.md#/orders/confirmation'
          quote: The checkout shall send the customer a confirmation email after the payment succeeds.
      req:payment-1:
        statement: After the payment succeeds, the checkout shall send the customer a confirmation email.
        entities: [ent:checkout]
        source:
          section: 'docs/payment.md#/payment/after-payment'
          quote: After the payment succeeds, the checkout shall send the customer a confirmation email.
assert:
  - diagnosticExists:
      rule: duplicate-requirement
      subjects: [req:orders-1, req:payment-1]
  - nodeExists:
      id: req:orders-1
  - nodeExists:
      id: req:payment-1
  - diagnosticAbsent:
      rule: contradiction
```
