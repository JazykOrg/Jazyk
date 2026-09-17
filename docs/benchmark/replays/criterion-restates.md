# A criterion that restates its requirement

A criterion whose normalized text equals the statement tests nothing the statement does
not already say. The refusal says what a criterion is instead.

```yaml
name: criterion-restates
description: A criterion that restates its requirement is refused, and a concrete one lands
tier: review
goal: {kind: derive-criteria, target: ent:shop}
given:
  docs:
    docs/shop.md: |
      # Shop

      The Shop sends a confirmation email when an Order is submitted.
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
    coverage:
      docs/shop.md#/shop: covered
steps:
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
      kind: positive
      statement: A Customer submitting an Order for one in-stock Product receives one confirmation email.
      provenance: {derived: {from: [req:shop-1], reasoning: one concrete submission and its observable outcome}}
    expect: {ok: true}
  - call: mark_goal_done
    args:
      goal: g:derive-criteria:ent:shop
      justification: The one behavior statement carries one concrete criterion.
      evidence: {requirements: [{id: req:shop-1, criteria: [crit:shop-1], verdict: complete}]}
  - call: done
    args: {summary: one concrete criterion}
assert:
  - criteriaCount: {requirement: req:shop-1, min: 1, max: 1}
```
