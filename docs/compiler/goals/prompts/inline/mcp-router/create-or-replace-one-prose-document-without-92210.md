---
purpose: >-
  Instruction fragment: Create or replace one prose document without compilation.
used_by:
  - "bootstrap/src/mcp/router.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Create or replace one prose document without compilation. Read first and send its hash as baseHash. Omit baseHash only to create a missing document. Preserves document-save dirtiness; does not release or run compile work.