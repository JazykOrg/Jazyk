---
workflow: >-
  A tool-server session opens a batch of work, supplies its instructions and optional
  skills, and eventually commits or discards the batch. These messages explain that
  session lifecycle and which work remains open.
used_by:
  - "bootstrap/src/mcp/lifecycle.rs"
---
blocked goals wait on a release (jazyk release generate for bind and generate rows) or a human answer (jazyk answer); they do not hold the verdict