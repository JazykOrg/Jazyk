# turn-edges

Grades edge declaration. A sub-system list ties each listed sub-system to its parent;
the requirement carrying that fact must declare the pair in `edges` so the derived
relationship exists. Items that are links still count: the item's text is a fact of
this document (see [enumerations](../../compiler/concepts/ears.md#enumerations)). The
link targets are also a junk trap: no path or filename may become an entity. See
[case format](../cases.md#case-format).

```yaml
name: turn-edges
description: A sub-system list becomes typed relationships, not just prose.
task:
  type: reconcile-doc
  target: docs/platform.md
given:
  docs:
    docs/platform.md: |
      # Platform

      ## Sub-systems

      The warehouse platform consists of the following sub-systems:

      - [User Management](./user.md) - accounts and authentication
      - [Inventory Management](./inventory.md) - stock levels and product data
assert:
  - entityExists:
      name: User Management
  - entityExists:
      name: Inventory Management
  - relationshipExists:
      a: Warehouse Platform
      b: User Management
  - relationshipExists:
      a: Warehouse Platform
      b: Inventory Management
  - entityAbsent:
      namePattern: '^--|/|\.md'
  - coverageSet:
      section: 'docs/platform.md#/platform/sub-systems'
      state: covered
```
