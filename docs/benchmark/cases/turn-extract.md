# turn-extract

Grades extraction sanity. The model gets a fresh two-section document and an empty
graph. It must extract the planted entities and the checkout requirement, create no junk
entities (paths, flags, filenames), and mark both sections covered. See
[case format](../cases.md#case-format).

```yaml
name: turn-extract
description: Extract the planted entities and requirement from a fresh document, no junk.
task:
  type: reconcile-doc
  target: docs/shop.md
given:
  docs:
    docs/shop.md: |
      # Shop

      ## Cart

      The Cart shall hold each Product a customer intends to buy.

      ## Checkout

      When the customer checks out, the system shall empty the Cart.
assert:
  - entityExists:
      name: Cart
  - entityExists:
      name: Product
  - entityAbsent:
      namePattern: '^--|/|\.md'
  - requirementExists:
      earsPattern: 'empt(y|ies|ied)'
      entity: Cart
  - coverageSet:
      section: 'docs/shop.md#/shop/cart'
      state: covered
  - coverageSet:
      section: 'docs/shop.md#/shop/checkout'
      state: covered
```
