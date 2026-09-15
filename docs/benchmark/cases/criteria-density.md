# criteria-density

Grades criteria density. A plain fact takes one criterion and no contrived negative; a
conditional takes its positive and its negative (see
[criteria](../../compiler/skills/criteria.md)). An org handbook once carried 251
criteria for 126 requirements, most of them such pairs.

```yaml
name: criteria-density
description: One criterion on a plain fact, the negative only on a conditional.
tier: review
goal:
  kind: derive-criteria
  target: ent:company
given:
  docs:
    docs/company.md: |
      # Company

      The company has four departments. A claim over 500 dollars needs the head of
      department's approval.
  graph:
    entities:
      ent:company:
        name: Company
        definition: The organization the handbook describes.
        mentions:
          - {section: docs/company.md#/company, quote: "The company has four departments."}
      ent:claim:
        name: Claim
        definition: An expense claim an employee files.
        mentions:
          - {section: docs/company.md#/company, quote: "A claim over 500 dollars needs the head of department's approval."}
    requirements:
      req:company-1:
        statement: The company has four departments.
        entities: [ent:company]
        source: {section: docs/company.md#/company, quote: "The company has four departments."}
        facets: [{facet: constraint, reasoning: a standing fact about the company}]
      req:company-2:
        statement: A claim over 500 dollars needs the head of department's approval.
        entities: [ent:company, ent:claim]
        source: {section: docs/company.md#/company, quote: "A claim over 500 dollars needs the head of department's approval."}
        facets: [{facet: behavior, reasoning: an approval on a condition}]
    coverage:
      docs/company.md#/company: covered
assert:
  - criteriaCount:
      requirement: req:company-1
      min: 1
      max: 1
  - criteriaCount:
      requirement: req:company-2
      min: 2
      max: 3
```
