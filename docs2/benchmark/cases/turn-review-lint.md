# turn-review-lint

Grades lint application. The fixture carries one project lint rule and one planted
violation (the em dash is the plant; case fixtures are excluded from the docs glob, so
it lints nothing but itself). The model must report the violation under the `lint`
rule and must not invent findings under other rules. See
[case format](../cases.md#case-format).

```yaml
name: turn-review-lint
description: Apply a project lint rule where it fires.
tier: review
task:
  type: review-entity
  target: ent:gadget
given:
  docs:
    docs/gadget.md: |
      # Gadget

      ## Overview

      The Gadget shall report its battery level — the operator reads it hourly.

      ## Power

      When the battery drops below ten percent, the Gadget shall enter low power mode.
  graph:
    entities:
      ent:gadget:
        name: Gadget
        definition: A battery-powered field device.
        mentions:
          - section: 'docs/gadget.md#/gadget/overview'
            quote: The Gadget shall report its battery level — the operator reads it hourly.
          - section: 'docs/gadget.md#/gadget/power'
            quote: When the battery drops below ten percent, the Gadget shall enter low power mode.
    requirements:
      req:gadget-1:
        ears: The Gadget shall report its battery level.
        entities: [ent:gadget]
        source:
          section: 'docs/gadget.md#/gadget/overview'
          quote: The Gadget shall report its battery level — the operator reads it hourly.
      req:gadget-2:
        ears: When the battery drops below ten percent, the Gadget shall enter low power mode.
        entities: [ent:gadget]
        source:
          section: 'docs/gadget.md#/gadget/power'
          quote: When the battery drops below ten percent, the Gadget shall enter low power mode.
  lint:
    warnings:
      - An em dash appears in prose. Use commas, periods, parentheses, or colons instead.
assert:
  - diagnosticExists:
      rule: lint
  - diagnosticAbsent:
      rule: contradiction
```
