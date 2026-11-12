---
workflow: >-
  The tool server exposes document reads and other project operations to an external
  agent. These texts explain the operation being offered, including how binary documents
  are read without treating them as editable plain text.
used_by:
  - "bootstrap/src/mcp/router.rs"
---
Read a project document as compiler text. DOCX and PDF include source locations and diagnostics. Never overwrite binary documents with text.