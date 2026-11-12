---
purpose: >-
  Instruction fragment: Search retained run logs for literal failure text before reading
  more log context.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Search retained run logs for literal failure text before reading more log context. Returns bounded matching lines with one-based line numbers, not read_run_log character offsets.