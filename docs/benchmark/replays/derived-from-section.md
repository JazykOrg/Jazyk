# A section named as derived.from

Told a heading is not quotable, a local model retried with the section as
`derived.from` and read that a section it had just loaded did not exist. The refusal
says `from` names graph nodes and a fact the section states enters as a quoted
mention.

```yaml
name: derived-from-section
description: derived.from naming a section is refused toward a quoted mention, never as a missing section
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
      provenance: {derived: {from: ["docs/warehouse.md#/warehouse"], reasoning: the page is about it}}
    expect: {rule: unknown-id, contains: 'a section.*mention'}
assert:
  - entityAbsent: {namePattern: '^Warehouse$'}
```
