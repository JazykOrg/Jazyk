# A requirement without classification

A requirement upserts with its statement, entities and verbatim source. It needs
no classification. Repeated extraction preserves the obligation identity.

```yaml
name: requirement-without-classification
description: Requirement creation needs no classification.
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
    expect: {ok: true}
  - call: upsert_requirement
    args:
      statement: Theo Brandt arrives in Port Alder on the afternoon ferry.
      entities: [ent:theo-brandt]
      section: docs/plot.md#/plot/chapter-1
      quote: Theo Brandt arrives in Port Alder on the afternoon ferry.
    expect: {ok: true}
  - call: set_coverage
    args: {section: docs/plot.md#/plot/chapter-1, state: covered, note: the arrival sentence is the chapter's one statement}
  - call: mark_goal_done
    args:
      goal: g:reconcile-section:docs/plot.md
      justification: The chapter's one sentence is recorded as an obligation about Theo Brandt.
  - call: done
    args: {summary: the arrival statement is saved}
assert:
  - requirementCount: {min: 1, max: 1}
  - requirementExists: {statementPattern: 'afternoon ferry', entity: Theo Brandt}
```
