---
purpose: >-
  Instruction fragment: an internal build is running (lease `{}`, heartbeated every 30s,
  expires {}s after the last heartbeat). await_changes returns when it clears.
used_by:
  - "bootstrap/src/mcp/lifecycle.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
an internal build is running (lease `{}`, heartbeated every 30s, expires {}s after the last heartbeat). await_changes returns when it clears. If you were spawned BY that build, its serving carries --build-token and never sees this error; seeing it means this serving is a bystander and the board is not yours yet