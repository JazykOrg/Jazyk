# A rewording that drops the obligation modal

A local model staged "the system empties the Cart" beside the existing "shall empty
the Cart" off one quote, and the fold minted `req:shop-2` instead of landing on
`req:shop-1`. A same-sentence rewording compares words with `shall`, `must`, and
`will` set aside and verb endings reduced, so it lands on the existing id; an added
`can` changes what is required and stays a separate requirement.

```yaml
name: modal-reword
description: A same-quote rewording without its modal lands on the existing id; can beside must stays separate
goal: {kind: reconcile-section, target: docs/shop.md}
given:
  docs:
    docs/shop.md: |
      # Shop

      ## Checkout

      When the customer checks out, the system shall empty the Cart.

      ## Orders

      The Customer must cancel the Order when asked, and the Customer can cancel the Order otherwise.
  graph:
    entities:
      ent:cart:
        name: Cart
        definition: What the customer checks out.
        mentions:
          - {section: docs/shop.md#/shop/checkout, quote: "When the customer checks out, the system shall empty the Cart."}
      ent:customer:
        name: Customer
        definition: A person who orders from the shop.
        mentions:
          - {section: docs/shop.md#/shop/orders, quote: "The Customer must cancel the Order when asked"}
    requirements:
      req:shop-1:
        statement: When the customer checks out, the system shall empty the Cart.
        entities: [ent:cart]
        source: {section: docs/shop.md#/shop/checkout, quote: "When the customer checks out, the system shall empty the Cart."}
        facets: [{facet: behavior, reasoning: the system acts on checkout}]
      req:shop-2:
        statement: The Customer must cancel the Order.
        entities: [ent:customer]
        source: {section: docs/shop.md#/shop/orders, quote: "The Customer must cancel the Order when asked, and the Customer can cancel the Order otherwise."}
        facets: [{facet: behavior, reasoning: the customer acts}]
steps:
  - call: upsert_requirement
    args:
      statement: When the customer checks out, the system empties the Cart.
      entities: [ent:cart]
      section: docs/shop.md#/shop/checkout
      quote: When the customer checks out, the system shall empty the Cart.
      facets: [{facet: behavior, reasoning: the system acts on checkout}]
    expect: {contains: '"id":"req:shop-1"'}
  - call: upsert_requirement
    args:
      statement: The Customer can cancel the Order.
      entities: [ent:customer]
      section: docs/shop.md#/shop/orders
      quote: The Customer must cancel the Order when asked, and the Customer can cancel the Order otherwise.
      facets: [{facet: behavior, reasoning: a permission of the customer}]
    expect: {absent: '"id":"req:shop-2"'}
  - call: set_coverage
    args: {section: docs/shop.md#/shop/checkout, state: covered, note: the one checkout statement}
  - call: set_coverage
    args: {section: docs/shop.md#/shop/orders, state: covered, note: the obligation and the permission}
  - call: mark_goal_done
    args:
      goal: g:reconcile-section:docs/shop.md
      justification: The checkout rewording lands on its requirement, and the permission to cancel is its own statement beside the obligation.
  - call: done
    args: {summary: one rewording folded, one permission added}
assert:
  - requirementCount: {min: 3, max: 3}
  - requirementExists: {statementPattern: 'can cancel', entity: Customer}
  - requirementExists: {statementPattern: 'must cancel', entity: Customer}
```
