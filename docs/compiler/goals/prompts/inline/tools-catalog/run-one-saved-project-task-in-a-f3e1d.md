---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Run one saved project task in a fresh isolated environment. Returns a durable run ID immediately; wait=true waits in the host and returns the terminal status, or the current status if interrupted (then use wait_run). No per-run args, inputs or environment overrides exist. Reuse a suite task for related checks; the verification tools import its independent JUnit case evidence. The command and fixed timeout cannot change mid-run.