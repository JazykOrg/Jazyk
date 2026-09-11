# curate-view

Grades view curation on a query match
([`curate-view`](../../compiler/goals/curate-view.md)). The goal derives from the
seeded fixture: a curated class view carries `query: {scope: public}`, and the seeding
commit's recompute finds two entities the query newly matches, joins them to the view,
and writes `query-match` on it ([derived goals](../cases.md#derived-goals)).

The two matches are the ground truth. `Refund` is a stated class of the scope and
belongs in the class view: the session confirms it. `Order 1042` is a worked example,
a stated instance with no instantiation edge yet, so the query picked it up; instances
belong in object views, not class views, and the session excludes it with a note. See
[placement](../../compiler/goals/curate-view.md#placement).

```yaml
name: curate-view
description: Confirm a query match that belongs in a class view and exclude a stated instance with a note.
tier: structure
par:
  rounds: 2
goal:
  kind: curate-view
  target: view:class/commerce
given:
  docs:
    docs/commerce.md: |
      # Commerce

      ## Order

      An order is a customer's purchase.

      ## Invoice

      An invoice bills an order.
    docs/refunds.md: |
      # Refunds

      ## Refund

      A refund returns money for an invoice.
    docs/examples.md: |
      # Examples

      ## Order 1042

      Order 1042 is a worked example: an order with three lines and one invoice.
  graph:
    entities:
      ent:order:
        name: Order
        definition: A customer's purchase.
        mentions:
          - section: 'docs/commerce.md#/commerce/order'
            quote: An order is a customer's purchase.
      ent:invoice:
        name: Invoice
        definition: Bills an order.
        mentions:
          - section: 'docs/commerce.md#/commerce/invoice'
            quote: An invoice bills an order.
      ent:refund:
        name: Refund
        definition: Returns money for an invoice.
        mentions:
          - section: 'docs/refunds.md#/refunds/refund'
            quote: A refund returns money for an invoice.
      ent:order-1042:
        name: Order 1042
        stereotype: instance
        definition: A worked example order with three lines and one invoice.
        mentions:
          - section: 'docs/examples.md#/examples/order-1042'
            quote: 'Order 1042 is a worked example: an order with three lines and one invoice.'
    requirements:
      req:commerce-1:
        statement: An order is a customer's purchase.
        entities: [ent:order]
        source:
          section: 'docs/commerce.md#/commerce/order'
          quote: An order is a customer's purchase.
      req:commerce-2:
        statement: An invoice bills an order.
        entities: [ent:invoice, ent:order]
        edges:
          - {a: ent:invoice, b: ent:order, type: dependency}
        source:
          section: 'docs/commerce.md#/commerce/invoice'
          quote: An invoice bills an order.
      req:refunds-1:
        statement: A refund returns money for an invoice.
        entities: [ent:refund, ent:invoice]
        edges:
          - {a: ent:refund, b: ent:invoice, type: dependency}
        source:
          section: 'docs/refunds.md#/refunds/refund'
          quote: A refund returns money for an invoice.
      req:examples-1:
        statement: 'Order 1042 is a worked example: an order with three lines and one invoice.'
        entities: [ent:order-1042]
        source:
          section: 'docs/examples.md#/examples/order-1042'
          quote: 'Order 1042 is a worked example: an order with three lines and one invoice.'
    views:
      view:class/commerce:
        kind: class
        title: Commerce
        members: [ent:order, ent:invoice]
        query:
          scope: public
        provenance:
          derived:
            from: [ent:order, ent:invoice]
            reasoning: The classes of the commerce scope, curated to follow the scope.
assert:
  - viewMember:
      view: view:class/commerce
      member: ent:refund
  - viewExcludes:
      view: view:class/commerce
      member: ent:order-1042
  - nodeExists:
      id: view:class/commerce
  - nodeExists:
      id: ent:order-1042
```
