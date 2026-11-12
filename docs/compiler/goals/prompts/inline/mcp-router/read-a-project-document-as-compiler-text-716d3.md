---
purpose: >-
  Instruction fragment: Read a project document as compiler text.
used_by:
  - "bootstrap/src/mcp/router.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Read a project document as compiler text. DOCX and PDF include source locations and diagnostics. Never overwrite binary documents with text.