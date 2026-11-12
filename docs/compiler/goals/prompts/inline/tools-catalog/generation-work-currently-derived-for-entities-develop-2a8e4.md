---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Generation work currently derived for entities. Develop implementation and tests together; a separate Bind pass is optional. An empty list does not prove that an artifact exists or its checks passed. Follow next and prerequisites, then begin_generation on one ready entity.