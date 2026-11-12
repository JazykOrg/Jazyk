---
purpose: >-
  Instruction fragment: Read a bounded run log slice, including after teardown.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Read a bounded run log slice, including after teardown. Offsets and limits count Unicode characters, not bytes or lines. Continue with next_offset; search match line numbers are not read offsets.