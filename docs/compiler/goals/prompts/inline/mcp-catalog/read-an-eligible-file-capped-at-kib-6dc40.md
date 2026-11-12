---
purpose: >-
  Instruction fragment: Read an eligible file, capped at 64 KiB. line is one-based and
  limit counts lines.
used_by:
  - "bootstrap/src/mcp/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Read an eligible file, capped at 64 KiB. line is one-based and limit counts lines. Private state and credentials are excluded.