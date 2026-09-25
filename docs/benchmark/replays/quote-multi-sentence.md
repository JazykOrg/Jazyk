# A requirement quoting two sentences

A local model merged the React and REST sentences of a warehouse page into one
statement quoting both (density 0.83). A requirement quote that spans two full
sentences is refused, naming them; each sentence quoted alone lands as its own
requirement.

```yaml
name: quote-multi-sentence
description: A requirement quoting two sentences is refused naming them; one requirement per sentence lands
goal: {kind: reconcile-section, target: docs/warehouse.md}
given:
  docs:
    docs/warehouse.md: |
      # Warehouse

      ## Frontend

      The frontend is a web application built using React. It communicates with the
      backend over a REST API.
  graph:
    entities:
      ent:frontend:
        name: Frontend
        definition: The web application the warehouse staff use.
        mentions:
          - {section: docs/warehouse.md#/warehouse/frontend, quote: "The frontend is a web application built using React."}
steps:
  - call: upsert_requirement
    args:
      statement: The Frontend is a React web application that talks to the backend over a REST API.
      entities: [ent:frontend]
      section: docs/warehouse.md#/warehouse/frontend
      quote: The frontend is a web application built using React. It communicates with the backend over a REST API.
      facets: [{facet: constraint, reasoning: the frontend's form}]
    expect: {rule: quote-multi-sentence, contains: '2 sentences'}
  - call: upsert_requirement
    args:
      statement: The Frontend is a web application built using React.
      entities: [ent:frontend]
      section: docs/warehouse.md#/warehouse/frontend
      quote: The frontend is a web application built using React.
      facets: [{facet: constraint, reasoning: the frontend's form}]
  - call: upsert_requirement
    args:
      statement: The Frontend communicates with the backend over a REST API.
      entities: [ent:frontend]
      section: docs/warehouse.md#/warehouse/frontend
      quote: It communicates with the backend over a REST API.
      facets: [{facet: constraint, reasoning: the frontend's interface}]
  - call: set_coverage
    args: {section: docs/warehouse.md#/warehouse/frontend, state: covered, note: one requirement per sentence}
  - call: mark_goal_done
    args:
      goal: g:reconcile-section:docs/warehouse.md
      justification: Each of the section's two sentences is its own requirement of the Frontend.
  - call: done
    args: {summary: two requirements, one per sentence}
assert:
  - requirementExists: {statementPattern: 'built using React', entity: Frontend}
  - requirementExists: {statementPattern: 'REST API', entity: Frontend}
  - requirementCount: {max: 2}
```
