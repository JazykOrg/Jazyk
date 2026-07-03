# turn-converge

Grades convergence discipline. The graph already reflects the document exactly and the
sections are already covered in the fixture. A capable model recognizes there is nothing
to do and stages zero mutations. See
[incremental builds](../../compiler/reconciler.md#incremental-builds).

```yaml
name: turn-converge
description: Stage zero mutations when the graph already reflects the document.
task:
  type: reconcile-doc
  target: docs/shop.md
given:
  docs:
    docs/shop.md: |
      # Shop

      ## Checkout

      When the customer checks out, the system shall empty the Cart.
  graph:
    entities:
      ent:cart:
        name: Cart
        definition: Holds the products a customer intends to buy.
        mentions:
          - section: 'docs/shop.md#/shop/checkout'
            quote: When the customer checks out, the system shall empty the Cart.
      ent:customer:
        name: Customer
        definition: A person who buys from the shop.
        mentions:
          - section: 'docs/shop.md#/shop/checkout'
            quote: When the customer checks out, the system shall empty the Cart.
    requirements:
      req:shop-1:
        ears: When the customer checks out, the system shall empty the Cart.
        entities: [ent:customer, ent:cart]
        source:
          section: 'docs/shop.md#/shop/checkout'
          quote: When the customer checks out, the system shall empty the Cart.
    coverage:
      'docs/shop.md#/shop': covered
      'docs/shop.md#/shop/checkout': covered
assert:
  - mutationCount:
      max: 0
```
