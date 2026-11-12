---
purpose: >-
  Instruction fragment: Resolve final goals and commit the staged mutations in one call.
  goals entries use {goal, justification, evidence?}, with the same evidence and gates as
  mark_goal_done.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Resolve final goals and commit the staged mutations in one call. goals entries use {goal, justification, evidence?}, with the same evidence and gates as mark_goal_done. Earlier claims may already be marked. Every mandatory goal must end done or failed; all batch gates run. A rejected claim or gate leaves earlier work intact for repair. summary is one line.