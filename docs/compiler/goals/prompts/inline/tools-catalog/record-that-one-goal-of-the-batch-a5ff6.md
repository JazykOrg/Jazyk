---
purpose: >-
  Instruction fragment: Record that one goal of the batch cannot honestly be accomplished,
  with a one or two sentence reason.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Record that one goal of the batch cannot honestly be accomplished, with a one or two sentence reason. Always accepted; a failed mandatory goal blocks convergence, a failed optional goal is recorded and stands.