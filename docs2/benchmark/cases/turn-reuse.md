# turn-reuse

Grades reuse discipline. The graph already holds `ent:customer`. The document mentions
the Customer again. The model must search before creating and land the new requirement
on the existing entity. The `entityCount` bound leaves room for a new `Order` entity but
not for a duplicate customer. See [case format](../cases.md#case-format).

```yaml
name: turn-reuse
description: Reuse the existing Customer entity instead of creating a duplicate.
task:
  type: reconcile-doc
  target: docs/orders.md
given:
  graph:
    entities:
      ent:customer:
        name: Customer
        definition: A person who buys from the shop.
  docs:
    docs/orders.md: |
      # Orders

      ## Placing an order

      When the Customer places an order, the system shall create an Order.
assert:
  - requirementExists:
      earsPattern: 'places an order'
      entity: ent:customer
  - entityCount:
      max: 2
```
