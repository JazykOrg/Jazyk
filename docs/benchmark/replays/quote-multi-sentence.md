# One obligation across consecutive sentences

A color obligation can use two consecutive sentences as one verbatim source.

```yaml
name: quote-multi-sentence
description: A requirement locates a multi-sentence source passage
goal: {kind: reconcile-section, target: docs/warehouse.md}
given:
  docs:
    docs/warehouse.md: |
      # Warehouse

      ## Frontend

      The background is blue. Use #1E40AF for it.
  graph:
    entities:
      ent:frontend:
        name: Frontend
        definition: The warehouse frontend.
        mentions:
          - {section: docs/warehouse.md#/warehouse/frontend, quote: "The background is blue. Use #1E40AF for it."}
steps:
  - call: upsert_requirement
    args:
      statement: "The Frontend background uses #1E40AF."
      entities: [ent:frontend]
      section: docs/warehouse.md#/warehouse/frontend
      quote: "The background is blue. Use #1E40AF for it."
      facets: [{facet: constraint, reasoning: the background color}]
  - call: set_coverage
    args: {section: docs/warehouse.md#/warehouse/frontend, state: covered, note: the background color obligation}
  - call: mark_goal_done
    args:
      goal: g:reconcile-section:docs/warehouse.md
      justification: One obligation captures the background color and its concrete value.
  - call: done
    args: {summary: one color requirement}
assert:
  - requirementExists: {statementPattern: '#1E40AF', entity: Frontend}
  - requirementCount: {max: 1}
```
