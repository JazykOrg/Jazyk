---
workflow: >-
  An external agent discovers Jazyk operations from the tool server before calling them.
  These texts describe those operations or their available capabilities; they are
  tool-facing instructions, not results of executing the operation.
used_by:
  - "bootstrap/src/mcp/catalog.rs"
---
Repair docs, source or roots matchers on this worker's current document set. Preserve its id and unrelated selections; use baseHash from list_document_sets. New Source patterns apply to the next file write.