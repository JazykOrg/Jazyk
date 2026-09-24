# criteria-observation

Grades what a criterion observes. For software, the observer uses the product the way
its user does: a client sends a request and reads the response. Inspecting a contract, a
specification, or a configuration is not an observation, and a criterion framed that way
lets a descriptor object pass for a server. A generated ERP once read verified on every
criterion with no server at all, its first criterion reading "a client examines the
integration contract" (see [criteria](../../compiler/skills/criteria.md)).

```yaml
name: criteria-observation
description: Criteria on a software requirement observe the running product, never a contract or a specification.
tier: review
goal:
  kind: derive-criteria
  target: ent:backend
given:
  docs:
    docs/backend.md: |
      # Backend

      The backend is a REST API built using Node.js and Express. The frontend lists the
      products in stock through the backend's API.
  graph:
    entities:
      ent:backend:
        name: Backend
        definition: The REST API the frontend calls.
        mentions:
          - {section: docs/backend.md#/backend, quote: "The backend is a REST API built using Node.js and Express."}
    requirements:
      req:backend-1:
        statement: The Backend is a REST API built using Node.js and Express.
        entities: [ent:backend]
        source: {section: docs/backend.md#/backend, quote: "The backend is a REST API built using Node.js and Express."}
        facets: [{facet: constraint, reasoning: an invariant of the backend's form}]
      req:backend-2:
        statement: The Frontend lists the products in stock through the Backend's API.
        entities: [ent:backend]
        source: {section: docs/backend.md#/backend, quote: "The frontend lists the products in stock through the backend's API."}
        facets: [{facet: behavior, reasoning: the frontend acts through the backend}]
    coverage:
      docs/backend.md#/backend: covered
assert:
  - criteriaMatching:
      pattern: 'contract|specification|spec\b|config|examin|inspect|document'
      max: 0
  - criteriaMatching:
      pattern: 'request|respon|endpoint|GET |POST |call'
      min: 1
  - criteriaCount:
      requirement: req:backend-2
      min: 1
```
