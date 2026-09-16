# properties

Grades property lists. A sentence listing several properties of one subject (a size, a
location, a count) is one statement carrying the values, each value also an attribute of
the subject; it is not one requirement per value (see
[granularity](../../compiler/concepts/statements.md#granularity)).

```yaml
name: properties
description: A sentence listing several properties of one subject is one statement with attributes.
goal:
  kind: reconcile-section
  target: docs/company.md
given:
  docs:
    docs/company.md: |
      # Ridgeline Outfitters

      Ridgeline Outfitters is an outdoor gear retailer with about 120 employees, a
      warehouse in Boulder, and four stores along the Front Range. Every store opens at
      nine.
assert:
  - entityExists:
      name: Ridgeline Outfitters
  - requirementCount:
      min: 2
      max: 3
  - requirementExists:
      statementPattern: 'opens at nine'
      entity: Store
  - attributeExists:
      entity: Ridgeline Outfitters
      name: 'employee'
  - attributeExists:
      entity: Ridgeline Outfitters
      name: 'warehouse|boulder'
  - coverageSet:
      section: 'docs/company.md#/ridgeline-outfitters'
      state: covered
```
