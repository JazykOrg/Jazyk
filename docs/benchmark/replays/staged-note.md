# Staged work on the loaded listing

A Codex session updated a requirement's statement, received `updated: true`, and read
the old statement in the listing that followed as a lost edit (D145). The listing
renders the committed snapshot, and a line a staged mutation touches says so.

```yaml
name: staged-note
description: A loaded item a staged mutation touches carries a staged note on its listing line
goal: {kind: reconcile-section, target: docs/shop.md}
given:
  docs:
    docs/shop.md: |
      # Shop

      ## Customer

      A Customer is a person who buys from the shop.
  graph:
    entities:
      ent:customer:
        name: Customer
        definition: A Customer is a person who buys.
        mentions:
          - {section: docs/shop.md#/shop/customer, quote: "A Customer is a person who buys from the shop."}
steps:
  - call: load
    args: {target: ent:customer}
  - call: update_entity
    args: {id: ent:customer, definition: A Customer is a person who buys from the shop.}
  - call: graph_status
    args: {}
    expect: {contains: "staged: revised"}
assert:
  - nodeExists: {id: ent:customer}
```
