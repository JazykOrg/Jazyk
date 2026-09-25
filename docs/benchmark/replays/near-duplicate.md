# A name variant of an existing entity

A bare name beside an existing one that carries a generic classifier is that entity under
another wording: the refusal names the id to reuse. A name that adds a concrete noun is
another concept and lands. The boundary between the two cases is what this replay holds,
and it is directional: a compound extending an existing single name is a new concept.

```yaml
name: near-duplicate
description: A bare name beside an existing classifier-carrying one is refused; a concrete noun is another concept
goal: {kind: reconcile-section, target: docs/ledger.md}
given:
  docs:
    docs/ledger.md: |
      # Ledger

      ## Payments

      The Payment Service is how a Customer pays. Payment clears within two days, and
      the Payment Report lists what cleared.
  graph:
    entities:
      ent:payment-service:
        name: Payment Service
        definition: How a Customer pays.
        mentions:
          - {section: docs/ledger.md#/ledger/payments, quote: "The Payment Service is how a Customer pays."}
steps:
  - call: upsert_entity
    args:
      name: Payment
      definition: The clearing of a customer's money.
      mention: {section: docs/ledger.md#/ledger/payments, quote: "Payment clears within two days, and"}
    expect: {rule: near-duplicate, contains: 'ent:payment-service'}
  - call: upsert_entity
    args:
      name: Payment Report
      definition: The report listing what cleared.
      mention: {section: docs/ledger.md#/ledger/payments, quote: "and the Payment Report lists what cleared."}
    expect: {ok: true}
  - call: upsert_requirement
    args:
      statement: The Payment Report lists what cleared.
      entities: [ent:payment-report]
      section: docs/ledger.md#/ledger/payments
      quote: and the Payment Report lists what cleared.
      facets: [{facet: constraint, reasoning: an invariant of the report}]
    expect: {ok: true}
  - call: set_coverage
    args: {section: docs/ledger.md#/ledger/payments, state: covered, note: the report sentence is the section's statement}
  - call: mark_goal_done
    args:
      goal: g:reconcile-section:docs/ledger.md
      justification: The report is its own concept and carries the section's statement; the bare Payment is the Payment Service under another wording.
  - call: done
    args: {summary: the report, and Payment reused}
assert:
  - entityNameCount: {namePattern: '^Payment$', max: 0}
  - entityExists: {name: Payment Report}
```
