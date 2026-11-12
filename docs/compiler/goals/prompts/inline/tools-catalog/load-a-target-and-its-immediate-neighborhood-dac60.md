---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Load a target and its immediate neighborhood into the loaded set: any node id (ent:..., req:..., view:..., diag:...), a section reference (doc.md#/ref), or alignment:<document> for pending anchor proposals and section changes. depth defaults to 1: the target in full, its edges, each neighbor as a stub; depth 2 loads the neighbors in full too. Whatever the budget cut off arrives as expansion handles (h:<target>:<axis>). Loading an already loaded target is a repeat; deepen a loaded node with expand instead.