# dedupe-candidates

Grades lookalike pair judgment
([`dedupe-candidates`](../../compiler/goals/dedupe-candidates.md)), in two cases. The
goal derives from the seeded fixture: the name index scores the pair at derivation,
and a score at the threshold derives the pair goal
([derived goals](../cases.md#derived-goals)). The gate checks that the pair was merged
or kept with a reason; only the planted ground truth grades which.

## One concept under two names

`backend` and `backend system`, each in its own document, score 0.5: one concept. The
session merges them, the absorbed name survives as an alias, and both requirements
survive on the survivor.

```yaml
name: dedupe-candidates
description: Merge two cross-document entities that are one concept, keeping the absorbed name as an alias and both requirements.
tier: review
par:
  rounds: 2
goal:
  kind: dedupe-candidates
  target: ent:backend~ent:backend-system
given:
  docs:
    docs/api.md: |
      # API

      ## Backend

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
          - section: 'docs/api.md#/api/backend'
            quote: The backend shall handle API requests and persistence.
      ent:backend-system:
        name: Backend System
        definition: The server side of the platform.
        mentions:
          - section: 'docs/deploy.md#/deployment/runtime'
            quote: The backend system shall run as a single container.
    requirements:
      req:api-1:
        statement: The backend shall handle API requests and persistence.
        entities: [ent:backend]
        source:
          section: 'docs/api.md#/api/backend'
          quote: The backend shall handle API requests and persistence.
      req:deploy-1:
        statement: The backend system shall run as a single container.
        entities: [ent:backend-system]
        source:
          section: 'docs/deploy.md#/deployment/runtime'
          quote: The backend system shall run as a single container.
assert:
  - entityCount:
      max: 1
  - entityExists:
      name: Backend
  - entityExists:
      name: Backend System
  - requirementCount:
      min: 2
```

## A shared word and nothing else

`Product` and `Product price` share a word and score 0.5 across their two documents.
Statements are directly about each as its own thing: the session keeps both. A merge
is the wrong call; a `duplicate-entity` finding beside the reason is allowed.

```yaml
name: dedupe-candidates-separate
description: Keep two lookalike entities apart when statements are directly about each as its own thing.
tier: review
par:
  rounds: 2
goal:
  kind: dedupe-candidates
  target: ent:product~ent:product-price
given:
  docs:
    docs/catalog.md: |
      # Catalog

      ## Product

      A product has a name and a SKU.
    docs/pricing.md: |
      # Pricing

      ## Product price

      The product price shall be stored in the customer's currency.
  graph:
    entities:
      ent:product:
        name: Product
        definition: An item the shop sells.
        mentions:
          - section: 'docs/catalog.md#/catalog/product'
            quote: A product has a name and a SKU.
      ent:product-price:
        name: Product Price
        definition: The amount charged for a product.
        mentions:
          - section: 'docs/pricing.md#/pricing/product-price'
            quote: The product price shall be stored in the customer's currency.
    requirements:
      req:catalog-1:
        statement: A product has a name and a SKU.
        entities: [ent:product]
        source:
          section: 'docs/catalog.md#/catalog/product'
          quote: A product has a name and a SKU.
      req:pricing-1:
        statement: The product price shall be stored in the customer's currency.
        entities: [ent:product-price]
        source:
          section: 'docs/pricing.md#/pricing/product-price'
          quote: The product price shall be stored in the customer's currency.
assert:
  - nodeExists:
      id: ent:product
  - nodeExists:
      id: ent:product-price
  - entityCount:
      min: 2
      max: 2
  - requirementCount:
      min: 2
```
