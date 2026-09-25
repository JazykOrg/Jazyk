# An entity minted from its page heading

A local model spent six of twenty-four text-codec rounds minting a Warehouse system the
body never names, by one route after another, each refused, and ran out one
requirement short. An entity quoting the page heading hears that an entity no body
sentence names is not minted from its heading.

```yaml
name: heading-entity
description: An entity quoting the page heading is refused and told it is not minted from its heading
goal: {kind: reconcile-section, target: docs/warehouse.md}
given:
  docs:
    docs/warehouse.md: |
      # Warehouse

      Pickers pack each order the day it arrives.
steps:
  - call: upsert_entity
    args:
      name: Warehouse
      stereotype: system
      definition: The system the page describes.
      mention: {section: docs/warehouse.md#/warehouse, quote: "Warehouse"}
    expect: {rule: quote-not-found, contains: 'not minted from its heading'}
  - call: upsert_entity
    args:
      name: Warehouse
      stereotype: system
      definition: The system the page describes.
      mention: {section: docs/warehouse.md#/warehouse, quote: "# Warehouse"}
    expect: {rule: quote-not-found, contains: 'not minted from its heading'}
assert:
  - entityAbsent: {namePattern: '^Warehouse$'}
```
