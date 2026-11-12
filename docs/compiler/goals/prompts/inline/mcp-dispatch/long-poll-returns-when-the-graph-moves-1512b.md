---
purpose: >-
  Instruction fragment: Long poll: returns when the graph moves, a documentation or source
  file changes (woken by file events), or the ledger or a watched deliverable file
  changes, or at the timeout (default 300s, at most 3600s.
used_by:
  - "bootstrap/src/mcp/dispatch.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Long poll: returns when the graph moves, a documentation or source file changes (woken by file events), or the ledger or a watched deliverable file changes, or at the timeout (default 300s, at most 3600s; 0 waits the full 3600s, use only when your client does not bound tool calls). Carries the task counts per queue and which tool lists the work. Never call it with a task open; finish with done first.