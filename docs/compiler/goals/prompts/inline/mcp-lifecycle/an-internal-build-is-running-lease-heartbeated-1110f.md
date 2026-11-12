---
workflow: >-
  A tool-server session opens a batch of work, supplies its instructions and optional
  skills, and eventually commits or discards the batch. These messages explain that
  session lifecycle and which work remains open.
used_by:
  - "bootstrap/src/mcp/lifecycle.rs"
---
an internal build is running (lease `{}`, heartbeated every 30s, expires {}s after the last heartbeat). await_changes returns when it clears. If you were spawned BY that build, its serving carries --build-token and never sees this error; seeing it means this serving is a bystander and the board is not yours yet