---
purpose: >-
  Instruction fragment: old_text requires expectedHash from the last read or write and
  cannot combine with append: true.
used_by:
  - "bootstrap/src/tools/files.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
old_text requires expectedHash from the last read or write and cannot combine with append: true; no bytes changed