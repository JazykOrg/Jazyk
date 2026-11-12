---
purpose: >-
  Instruction fragment: `{rel}` is recorded for entity `ent:{owner}`.
used_by:
  - "bootstrap/src/tools/files.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
`{rel}` is recorded for entity `ent:{owner}`. Call begin_generation for that entity before repairing its files; successful record_generation ends the previous claim.