---
purpose: >-
  Instruction fragment: Project files edited since the cursor `since` (since this serving
  started when absent), by any tool: your own edit tools, an editor, git.
used_by:
  - "bootstrap/src/mcp/watch.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Project files edited since the cursor `since` (since this serving started when absent), by any tool: your own edit tools, an editor, git. Edit files with your own tools; this shows their effect before any build. A document row lists the open compile goals its edit derived (dirtied sections); a source row names the entity owning the file. Pass the reply's cursor as the next since. Read only: never compiles.