---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Load skills from the skills index. Use names for several independent lookups in one call, or name for one skill. Each batch entry returns its payload or its own error. Already delivered skill text is not repeated. The session skill budget limits how many distinct skills can render.
