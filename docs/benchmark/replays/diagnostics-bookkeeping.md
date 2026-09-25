# Bookkeeping in the unfiltered diagnostics listing

A novel compile's unfiltered `diagnostics` listing carried 222 `ratification-pending`
entries, one per derived criterion, and Codex read a truncated wall that hid its three
judgment findings. Unfiltered, the build's bookkeeping is counted per rule under
`bookkeeping`, not listed; passing the rule lists it. The fixture seeds through one
commit, so the derived Shop files its ratification proposal as a build would.

```yaml
name: diagnostics-bookkeeping
description: Unfiltered diagnostics counts ratification-pending under bookkeeping; rule lists it
goal: {kind: declare-edges, target: req:orders-1}
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
          - {section: 'docs/orders.md#/orders/structure', quote: The order service consists of the cart module and the pricing module.}
      ent:cart-module:
        name: Cart Module
        stereotype: module
        definition: The module that holds a customer's cart.
        mentions:
          - {section: 'docs/orders.md#/orders/structure', quote: the cart module}
      ent:pricing-module:
        name: Pricing Module
        stereotype: module
        definition: The module that computes totals.
        mentions:
          - {section: 'docs/orders.md#/orders/structure', quote: the pricing module}
      ent:shop:
        name: Shop
        stereotype: system
        definition: The system the order service belongs to.
        provenance: {derived: {from: [ent:order-service], reasoning: a service belongs to a system the documents leave unnamed}}
    requirements:
      req:orders-1:
        statement: The order service consists of the cart module and the pricing module.
        entities: [ent:order-service, ent:cart-module, ent:pricing-module]
        source: {section: 'docs/orders.md#/orders/structure', quote: The order service consists of the cart module and the pricing module.}
steps:
  - call: diagnostics
    args: {}
    expect: {contains: '"bookkeeping":\{[^}]*"ratification-pending":1', absent: '"rule":"ratification-pending"'}
  - call: diagnostics
    args: {rule: ratification-pending}
    expect: {contains: '"rule":"ratification-pending".*"subjects":\["ent:shop"\]', absent: bookkeeping}
assert:
  - diagnosticExists: {rule: ratification-pending}
```
