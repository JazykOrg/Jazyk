---
workflow: >-
  The embedded agent runtime decides which proposed tool calls can run and how to resume
  interrupted work. These messages distinguish calls that ran from calls that were
  skipped, blocked, or left with an uncertain outcome.
used_by:
  - "bootstrap/src/harness/runtime.rs"
---
Skipped: new human input arrived before dispatch; this call did not execute