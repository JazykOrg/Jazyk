---
purpose: >-
  Instruction fragment: Drop the open changeset without committing.
used_by:
  - "bootstrap/src/mcp/dispatch.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Drop the open changeset without committing. The batch's goals return to open.