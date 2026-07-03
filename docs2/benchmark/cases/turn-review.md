# turn-review

Grades review judgment, in two cases. The first plants a contradiction on `ent:abc`
across two documents; the model must report a `contradiction` diagnostic on it. The
second gives `ent:xyz` two compatible requirements; the model must stay quiet. Passing
both means the model judges, not pattern-matches. See
[case format](../cases.md#case-format).

## Contradiction

```yaml
name: turn-review
description: Flag a planted contradiction between two requirements on one entity.
task:
  type: review-entity
  target: ent:abc
given:
  docs:
    docs/frame.md: |
      # Frame

      ## ABC

      The ABC shall have exactly three wheels.
    docs/wheels.md: |
      # Wheels

      ## ABC

      The ABC shall have four wheels.
  graph:
    entities:
      ent:abc:
        name: ABC
        definition: A small vehicle.
        mentions:
          - section: 'docs/frame.md#/frame/abc'
            quote: The ABC shall have exactly three wheels.
          - section: 'docs/wheels.md#/wheels/abc'
            quote: The ABC shall have four wheels.
    requirements:
      req:frame-1:
        ears: The ABC shall have exactly three wheels.
        entities: [ent:abc]
        source:
          section: 'docs/frame.md#/frame/abc'
          quote: The ABC shall have exactly three wheels.
      req:wheels-1:
        ears: The ABC shall have four wheels.
        entities: [ent:abc]
        source:
          section: 'docs/wheels.md#/wheels/abc'
          quote: The ABC shall have four wheels.
assert:
  - diagnosticExists:
      rule: contradiction
      subject: ent:abc
```

## Clean input

```yaml
name: turn-review-clean
description: Stay quiet when an entity's requirements are compatible.
task:
  type: review-entity
  target: ent:xyz
given:
  docs:
    docs/xyz.md: |
      # XYZ

      ## Wheels

      The XYZ shall have four wheels.

      ## Parking

      While parked, the XYZ shall engage the parking brake.
  graph:
    entities:
      ent:xyz:
        name: XYZ
        definition: A four-wheeled vehicle.
        mentions:
          - section: 'docs/xyz.md#/xyz/wheels'
            quote: The XYZ shall have four wheels.
          - section: 'docs/xyz.md#/xyz/parking'
            quote: While parked, the XYZ shall engage the parking brake.
    requirements:
      req:xyz-1:
        ears: The XYZ shall have four wheels.
        entities: [ent:xyz]
        source:
          section: 'docs/xyz.md#/xyz/wheels'
          quote: The XYZ shall have four wheels.
      req:xyz-2:
        ears: While parked, the XYZ shall engage the parking brake.
        entities: [ent:xyz]
        source:
          section: 'docs/xyz.md#/xyz/parking'
          quote: While parked, the XYZ shall engage the parking brake.
assert:
  - diagnosticAbsent:
      rule: contradiction
```
