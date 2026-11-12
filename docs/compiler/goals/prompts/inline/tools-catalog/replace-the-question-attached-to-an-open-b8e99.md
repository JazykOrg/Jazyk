---
purpose: >-
  Instruction fragment: Replace the question attached to an open diagnostic (null prompt
  removes it).
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Replace the question attached to an open diagnostic (null prompt removes it). The finding itself is edited by re-reporting it; this tool only maintains the prompt. Never touches a human answer or triage.