# An entity minted from a sentence that never names it

A local model minted a Shop system twice from "the system shall empty the Cart" under a
page headed Shop. A created entity whose quote carries no word of its name or an alias
is refused; an entity the quote names lands.

```yaml
name: mention-unnamed
description: A new entity whose quote never names it is refused; one the quote names lands
goal: {kind: reconcile-section, target: docs/shop.md}
given:
  docs:
    docs/shop.md: |
      # Shop

      ## Checkout

      When the customer checks out, the system shall empty the Cart.
steps:
  - call: upsert_entity
    args:
      name: Shop
      stereotype: system
      definition: The system that runs the checkout.
      mention: {section: docs/shop.md#/shop/checkout, quote: "When the customer checks out, the system shall empty the Cart."}
    expect: {rule: mention-unnamed, contains: 'never names `Shop`'}
  - call: upsert_entity
    args:
      name: Cart
      definition: What the customer checks out.
      mention: {section: docs/shop.md#/shop/checkout, quote: "When the customer checks out, the system shall empty the Cart."}
assert:
  - entityAbsent: {namePattern: '^Shop$'}
```
