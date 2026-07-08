# turn-review-lookalike

Grades lookalike entity merge. Two documents minted `backend` and `backend system` as
separate entities for one concept. The review must merge them (the absorbed name
survives as an alias, requirements rewire to the survivor), not merely note the
resemblance. Both requirements must survive the merge. See
[case format](../cases.md#case-format).

```yaml
name: turn-review-lookalike
description: Merge two entities that are one concept, keeping both requirements.
tier: review
task:
  type: review-entity
  target: ent:backend
given:
  docs:
    docs/backend.md: |
      # Backend

      ## Role

      The backend shall handle API requests and persistence.
    docs/deploy.md: |
      # Deployment

      ## Runtime

      The backend system shall run as a single container.
  graph:
    entities:
      ent:backend:
        name: Backend
        definition: The server-side application.
        mentions:
          - section: 'docs/backend.md#/backend/role'
            quote: The backend shall handle API requests and persistence.
      ent:backend-system:
        name: Backend System
        definition: The server side of the platform.
        mentions:
          - section: 'docs/deploy.md#/deployment/runtime'
            quote: The backend system shall run as a single container.
    requirements:
      req:backend-1:
        ears: The backend shall handle API requests and persistence.
        entities: [ent:backend]
        source:
          section: 'docs/backend.md#/backend/role'
          quote: The backend shall handle API requests and persistence.
      req:deploy-1:
        ears: The backend system shall run as a single container.
        entities: [ent:backend-system]
        source:
          section: 'docs/deploy.md#/deployment/runtime'
          quote: The backend system shall run as a single container.
assert:
  - entityCount:
      max: 1
  - requirementCount:
      min: 2
  - diagnosticAbsent:
      rule: contradiction
```
