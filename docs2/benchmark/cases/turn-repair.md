# turn-repair

Grades repair. The graph holds the cart under `ent:shopping-cart`, not `ent:cart`. A
model that fabricates `ent:cart` in its requirement call is rejected by the
[validation gates](../../compiler/graph.md#validation-gates), and the rejection names
the nearest existing id. The case passes only if the model reads the rejection (or
searches first) and lands the requirement on `ent:shopping-cart`.

```yaml
name: turn-repair
description: Recover from a rejected call and reference the existing entity id.
task:
  type: reconcile-doc
  target: docs/store.md
given:
  graph:
    entities:
      ent:shopping-cart:
        name: Shopping Cart
        aliases: [cart]
        definition: Holds items a customer intends to buy.
  docs:
    docs/store.md: |
      # Store

      ## Checkout

      When the customer checks out, the system shall empty the cart.
assert:
  - requirementExists:
      earsPattern: 'empt(y|ies|ied)'
      entity: ent:shopping-cart
  - mutationCount:
      min: 1
```
