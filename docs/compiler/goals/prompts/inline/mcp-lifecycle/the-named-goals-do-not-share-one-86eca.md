---
workflow: >-
  A tool-server session opens a batch of work, supplies its instructions and optional
  skills, and eventually commits or discards the batch. These messages explain that
  session lifecycle and which work remains open.
used_by:
  - "bootstrap/src/mcp/lifecycle.rs"
---
the named goals do not share one ready batch (one locality, one executor); pick from the batches above