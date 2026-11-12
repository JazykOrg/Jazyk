---
purpose: >-
  Instruction fragment: Replace the question attached to an open diagnostic (null prompt
  removes it).
used_by:
  - "bootstrap/src/mcp/dispatch.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Replace the question attached to an open diagnostic (null prompt removes it). Never touches a human answer or triage.