---
purpose: >-
  Instruction fragment: Repair docs, source or roots matchers on this worker's current
  document set.
used_by:
  - "bootstrap/src/mcp/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Repair docs, source or roots matchers on this worker's current document set. Preserve its id and unrelated selections; use baseHash from list_document_sets. New Source patterns apply to the next file write.