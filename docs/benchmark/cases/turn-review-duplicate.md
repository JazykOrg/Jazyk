# turn-review-duplicate

Grades rephrase-duplicate collapse. One source sentence was extracted twice into two
statements wearing different words. Their token overlap sits below the deterministic
`duplicate-requirement` threshold, so only judgment can see they state one fact. The
model must keep one and delete the other, and must not misread the pair as a
contradiction. See [case format](../cases.md#case-format).

```yaml
name: turn-review-duplicate
description: Collapse two rewordings of one fact into a single requirement.
tier: review
task:
  type: review-entity
  target: ent:order-record
given:
  docs:
    docs/orders.md: |
      # Orders

      ## Retention

      Order records are kept for seven years.
  graph:
    entities:
      ent:order-record:
        name: Order Record
        definition: The persisted record of a completed order.
        mentions:
          - section: 'docs/orders.md#/orders/retention'
            quote: Order records are kept for seven years.
    requirements:
      req:orders-1:
        ears: The system shall retain order records for seven years.
        entities: [ent:order-record]
        source:
          section: 'docs/orders.md#/orders/retention'
          quote: Order records are kept for seven years.
      req:orders-2:
        ears: Order records shall be preserved by the system for a period of seven years.
        entities: [ent:order-record]
        source:
          section: 'docs/orders.md#/orders/retention'
          quote: Order records are kept for seven years.
assert:
  - requirementCount:
      entity: ent:order-record
      max: 1
  - requirementCount:
      entity: ent:order-record
      min: 1
  - diagnosticAbsent:
      rule: contradiction
```
