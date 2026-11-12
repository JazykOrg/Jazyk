---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Read one section's raw body and its child section titles. ref is a full section reference (doc.md#/ref). The body is capped at 400 lines and 16 KiB from line (default 1); a cut body carries truncated, totalLines, and nextLine.