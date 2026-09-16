# A requirement without a facet

A novel's plot document left 40 of its 55 statements facetless, so its story joined no
flow, gave no branch, and shaped no test, with nothing to say so. A create without a
facet is refused, naming the four kinds; an empty list is absent, the same case.

```yaml
name: facets-missing
description: A requirement create with no facet is refused, naming the four kinds
goal: {kind: reconcile-section, target: docs/plot.md}
given:
  docs:
    docs/plot.md: |
      # Plot

      ## Chapter 1

      Theo Brandt arrives in Port Alder on the afternoon ferry.
  graph:
    entities:
      ent:theo-brandt:
        name: Theo Brandt
        definition: A marine surveyor who arrives to survey the harbor wall.
        mentions:
          - {section: docs/plot.md#/plot/chapter-1, quote: "Theo Brandt arrives in Port Alder on the afternoon ferry."}
steps:
  - call: upsert_requirement
    args:
      statement: Theo Brandt arrives in Port Alder on the afternoon ferry.
      entities: [ent:theo-brandt]
      section: docs/plot.md#/plot/chapter-1
      quote: Theo Brandt arrives in Port Alder on the afternoon ferry.
    expect: {rule: facets-missing, contains: behavior}
  - call: upsert_requirement
    args:
      statement: Theo Brandt arrives in Port Alder on the afternoon ferry.
      entities: [ent:theo-brandt]
      section: docs/plot.md#/plot/chapter-1
      quote: Theo Brandt arrives in Port Alder on the afternoon ferry.
      facets: []
    expect: {rule: facets-missing}
  - call: upsert_requirement
    args:
      statement: Theo Brandt arrives in Port Alder on the afternoon ferry.
      entities: [ent:theo-brandt]
      section: docs/plot.md#/plot/chapter-1
      quote: Theo Brandt arrives in Port Alder on the afternoon ferry.
      facets: [{facet: behavior, reasoning: the character acts on the page}]
    expect: {ok: true}
assert:
  - requirementExists: {statementPattern: 'afternoon ferry', entity: Theo Brandt}
```
