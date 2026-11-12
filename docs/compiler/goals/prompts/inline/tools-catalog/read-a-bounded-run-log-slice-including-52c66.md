---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Read a bounded run log slice, including after teardown. Offsets and limits count Unicode characters, not bytes or lines. Continue with next_offset; search match line numbers are not read offsets.