# turn-density

Grades extraction density. The fixture is plain declarative prose: a technology
choice, a communication path, an access rule, and an operations list, none of it
using `shall`. The model must extract one atomic requirement per fact (six in total),
turn no operation name into an entity, and claim both sections covered. A cautious
model that waves the prose through as non-normative fails here, not in a build. See
[case format](../cases.md#case-format).

```yaml
name: turn-density
description: Extract atomic requirements from plain declarative prose at full recall.
task:
  type: reconcile-doc
  target: docs/warehouse.md
given:
  docs:
    docs/warehouse.md: |
      # Warehouse

      ## Frontend

      The frontend is a web application built using React. It communicates with the
      backend over a REST API. Only authenticated users can access the dashboard.

      ## Operations

      The inventory system supports the following operations:

      - `addProduct` - adds a new product to the inventory
      - `removeProduct` - removes a product from the inventory
      - `adjustStock` - corrects the stock count after an audit
assert:
  - requirementCount:
      min: 6
  - requirementExists:
      earsPattern: 'react'
      entity: Frontend
  - requirementExists:
      earsPattern: 'addProduct'
      entity: Inventory System
  - entityAbsent:
      namePattern: '^--|/|\.md|^addProduct$|^removeProduct$|^adjustStock$'
  - coverageSet:
      section: 'docs/warehouse.md#/warehouse/frontend'
      state: covered
  - coverageSet:
      section: 'docs/warehouse.md#/warehouse/operations'
      state: covered
```
