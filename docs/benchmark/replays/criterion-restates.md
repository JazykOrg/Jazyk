# A criterion that restates its requirement

A criterion whose normalized text equals the statement tests nothing the statement does
not already say, and one that equals another requirement's statement is that other
requirement (a local model gave `req:catalog-3` a negative that was `req:catalog-4` word
for word). The refusal says what a criterion is instead.

```yaml
name: criterion-restates
description: A criterion that restates its requirement is refused, and a concrete one lands
tier: review
goal: {kind: derive-criteria, target: ent:shop}
given:
  docs:
    docs/shop.md: |
      # Shop

      The Shop sends a confirmation email when an Order is submitted. When the Stock of a
      Product reaches zero, the Shop hides the Product.
  graph:
    entities:
      ent:shop:
        name: Shop
        definition: The shop the documents describe.
        mentions:
          - {section: docs/shop.md#/shop, quote: "The Shop sends a confirmation email when an Order is submitted."}
    requirements:
      req:shop-1:
        statement: The Shop sends a confirmation email when an Order is submitted.
        entities: [ent:shop]
        source: {section: docs/shop.md#/shop, quote: "The Shop sends a confirmation email when an Order is submitted."}
        facets: [{facet: behavior, reasoning: the shop acts on an event}]
      req:shop-2:
        statement: When the Stock of a Product reaches zero, the Shop hides the Product.
        entities: [ent:shop]
        source: {section: docs/shop.md#/shop, quote: "When the Stock of a Product reaches zero, the Shop hides the Product."}
        facets: [{facet: behavior, reasoning: the shop acts on an event}]
    coverage:
      docs/shop.md#/shop: covered
steps:
  - call: graph_status
    args: {}
    expect: {contains: 'docs/shop.md#/shop\s+section body'}
  - call: load
    args: {target: req:shop-1}
  - call: upsert_criterion
    args:
      requirement: req:shop-1
      kind: positive
      statement: The Shop sends a confirmation email when an Order is submitted.
      provenance: {derived: {from: [req:shop-1], reasoning: the stated behavior}}
    expect: {rule: criterion-restates, contains: 'concrete condition'}
  - call: upsert_criterion
    args:
      requirement: req:shop-1
      kind: negative
      statement: When the Stock of a Product reaches zero, the Shop hides the Product.
      provenance: {derived: {from: [req:shop-1], reasoning: another rule copied in}}
    expect: {rule: criterion-restates, contains: 'req:shop-2'}
  - call: upsert_criterion
    args:
      requirement: req:shop-1
      kind: positive
      statement: A Customer submitting an Order for one in-stock Product receives one confirmation email.
      provenance: {derived: {from: [req:shop-1], reasoning: one concrete submission and its observable outcome}}
    expect: {ok: true}
  - call: upsert_criterion
    args:
      requirement: req:shop-2
      kind: positive
      statement: A Product whose Stock drops from one to zero no longer shows in the Shop.
      provenance: {derived: {from: [req:shop-2], reasoning: one concrete stock-out and its observable outcome}}
    expect: {ok: true}
  - call: mark_goal_done
    args:
      goal: g:derive-criteria:ent:shop
      justification: Each behavior statement carries one concrete criterion.
      evidence: {requirements: [{id: req:shop-1, criteria: [crit:shop-1], verdict: complete}, {id: req:shop-2, criteria: [crit:shop-2], verdict: complete}]}
  - call: done
    args: {summary: one concrete criterion}
assert:
  - criteriaCount: {requirement: req:shop-1, min: 1, max: 1}
```
