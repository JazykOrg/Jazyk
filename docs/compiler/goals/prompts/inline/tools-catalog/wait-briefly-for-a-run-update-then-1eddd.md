---
purpose: >-
  Instruction fragment: Wait briefly for a run update, then return its durable status.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Wait briefly for a run update, then return its durable status. Repeating does not extend its deadline.