---
workflow: >-
  The embedded agent runtime decides which proposed tool calls can run and how to resume
  interrupted work. These messages distinguish calls that ran from calls that were
  skipped, blocked, or left with an uncertain outcome.
used_by:
  - "bootstrap/src/harness/runtime.rs"
---
execution-uncertain: Tool call '{id}' replays with arguments that differ from its recorded receipt; reconcile it before resuming