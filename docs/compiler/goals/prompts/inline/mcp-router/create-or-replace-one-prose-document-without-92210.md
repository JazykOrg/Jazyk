---
workflow: >-
  The tool server exposes document reads and other project operations to an external
  agent. These texts explain the operation being offered, including how binary documents
  are read without treating them as editable plain text.
used_by:
  - "bootstrap/src/mcp/router.rs"
---
Create, replace, or edit one prose document without compilation. For a change to an existing document, prefer a literal edit: old_text copied exactly from read_document (matching once, or every match with replaceAll true) and text as its replacement; baseHash then defaults to the hash of your last read or write of this path. A complete replacement sends the whole document as text with baseHash from read_document. Omit baseHash only to create a missing document. Preserves document-save dirtiness; does not release or run compile work.