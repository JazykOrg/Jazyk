---
purpose: >-
  Instruction fragment: option {} carries both edit and answer.
used_by:
  - "bootstrap/src/tools/diagnostics.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
option {} carries both edit and answer; keep one: `edit` when the option changes the document (the edit is its answer), `answer` when it replies in words; drop the other and send the call again