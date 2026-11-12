---
workflow: >-
  The tool server exposes document reads and other project operations to an external
  agent. These texts explain the operation being offered, including how binary documents
  are read without treating them as editable plain text.
used_by:
  - "bootstrap/src/mcp/router.rs"
---
Create or replace one prose document without compilation. Read first and send its hash as baseHash. Omit baseHash only to create a missing document. Preserves document-save dirtiness; does not release or run compile work.