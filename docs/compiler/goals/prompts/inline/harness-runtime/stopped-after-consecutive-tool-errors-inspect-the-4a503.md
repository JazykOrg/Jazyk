---
workflow: >-
  The embedded agent runtime decides which proposed tool calls can run and how to resume
  interrupted work. These messages distinguish calls that ran from calls that were
  skipped, blocked, or left with an uncertain outcome.
used_by:
  - "bootstrap/src/harness/runtime.rs"
---
Stopped after {} consecutive tool errors; inspect the failed calls before retrying