# enumeration

Grades a parts list. A lead-in stating containment over four named departments is one
membership obligation: one statement quoting the lead-in with a `composition` edge from
the whole to each part, each part's `parent` set to the whole, and each item's own
description its own statement quoting the item. An org corpus once yielded nine
statements here where five carry the same information (see
[enumerations](../../compiler/concepts/statements.md#enumerations)).

```yaml
name: enumeration
description: A parts list is one membership statement with an edge per part, and each part's own description its own statement.
goal:
  kind: reconcile-section
  target: docs/company.md
given:
  docs:
    docs/company.md: |
      # Ridgeline Outfitters

      ## The company

      Ridgeline Outfitters is made up of four departments:

      - Operations, which runs the warehouse and the stores.
      - Sales, which owns revenue and the wholesale accounts.
      - Finance, which keeps the books and runs payroll.
      - People, which hires and supports employees.
assert:
  - entityExists:
      name: Ridgeline Outfitters
  - entityExists:
      name: Operations
  - entityExists:
      name: People
  - parentIs:
      entity: Sales
      parent: Ridgeline Outfitters
  - relationshipExists:
      a: Ridgeline Outfitters
      b: Finance
  - requirementCount:
      min: 5
      max: 6
  - requirementExists:
      statementPattern: 'four departments'
      entity: Ridgeline Outfitters
  - requirementExists:
      statementPattern: 'payroll'
      entity: Finance
  - coverageSet:
      section: 'docs/company.md#/ridgeline-outfitters/the-company'
      state: covered
```
