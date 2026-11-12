---
purpose: >-
  Instruction fragment: option {} carries neither edit nor answer.
used_by:
  - "bootstrap/src/tools/diagnostics.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
option {} carries neither edit nor answer; give it one (`edit` to change the document, `answer` to reply in words); an option with neither is a filled-in blank, and a diagnostic that asks nothing of a human omits `prompt` entirely