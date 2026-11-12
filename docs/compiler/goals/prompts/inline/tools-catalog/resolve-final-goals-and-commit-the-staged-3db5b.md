---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Resolve final goals and commit the staged mutations in one call. goals entries use {goal, justification, evidence?}, with the same evidence and gates as mark_goal_done. Earlier claims may already be marked. Every mandatory goal must end done or failed; all batch gates run. A rejected claim or gate leaves earlier work intact for repair. summary is one line.