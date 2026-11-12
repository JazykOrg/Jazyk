---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
List retained file changes or read one changed path (version: changes, the default). version: input requires path and reads the frozen run input, including an unchanged file, explicitly labeled historical input. It does not read the current workspace. Missing contents and partial collection are explicit.