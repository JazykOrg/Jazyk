---
workflow: >-
  An external agent discovers Jazyk operations from the tool server before calling them.
  These texts describe those operations or their available capabilities; they are
  tool-facing instructions, not results of executing the operation.
used_by:
  - "bootstrap/src/mcp/catalog.rs"
---
Read an eligible file, capped at 64 KiB. line is one-based and limit counts lines. Private state and credentials are excluded.