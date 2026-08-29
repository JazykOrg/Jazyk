# verify-judge

Grades verification judgment against planted ground truth, as a pair. The model judges
the same criteria twice: once against an implementing file that satisfies the
statement (must come back `pass`), once against one that violates it (must come back
`fail`). The pair is the point: a sycophant that passes everything flunks the second
case, and a paranoiac that fails everything flunks the first. See
[case format](../cases.md#case-format).

```yaml
name: verify-judge-pass
description: Judge a satisfied criteria file pass.
tier: verification
par:
  rounds: 1
task:
  type: verify-requirement
  target: req:motd-1
given:
  docs:
    docs/motd.md: |
      # MOTD

      The MOTD file greets with exactly `Welcome aboard.` on its first line.
  graph:
    entities:
      ent:motd:
        name: MOTD
        mentions:
          - section: docs/motd.md#/motd
            quote: The MOTD file greets with exactly `Welcome aboard.` on its first line.
    requirements:
      req:motd-1:
        statement: The MOTD file shall greet with exactly `Welcome aboard.` on its first line.
        entities: [ent:motd]
        source:
          section: docs/motd.md#/motd
          quote: The MOTD file greets with exactly `Welcome aboard.` on its first line.
    coverage:
      docs/motd.md#/motd: covered
  deliverable:
    motd.txt: |
      Welcome aboard.
      Enjoy the ride.
assert:
  - verdictIs:
      requirement: req:motd-1
      verdict: pass
```

```yaml
name: verify-judge-fail
description: Judge a violated criteria file fail.
tier: verification
par:
  rounds: 1
task:
  type: verify-requirement
  target: req:motd-1
given:
  docs:
    docs/motd.md: |
      # MOTD

      The MOTD file greets with exactly `Welcome aboard.` on its first line.
  graph:
    entities:
      ent:motd:
        name: MOTD
        mentions:
          - section: docs/motd.md#/motd
            quote: The MOTD file greets with exactly `Welcome aboard.` on its first line.
    requirements:
      req:motd-1:
        statement: The MOTD file shall greet with exactly `Welcome aboard.` on its first line.
        entities: [ent:motd]
        source:
          section: docs/motd.md#/motd
          quote: The MOTD file greets with exactly `Welcome aboard.` on its first line.
    coverage:
      docs/motd.md#/motd: covered
  deliverable:
    motd.txt: |
      Hello there, traveler.
      Enjoy the ride.
assert:
  - verdictIs:
      requirement: req:motd-1
      verdict: fail
```
