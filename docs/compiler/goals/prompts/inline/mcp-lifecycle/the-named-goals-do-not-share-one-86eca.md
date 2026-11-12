---
purpose: >-
  Instruction fragment: the named goals do not share one ready batch (one locality, one
  executor).
used_by:
  - "bootstrap/src/mcp/lifecycle.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
the named goals do not share one ready batch (one locality, one executor); pick from the batches above