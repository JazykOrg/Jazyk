---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Move one proposed anchor (a requirement or an entity mention named in this align task) to the section that now holds its text. quote, when given, must locate verbatim there and replaces the stored quote. reevaluate true lists the anchor for the extraction turn to re-judge; false keeps it as an unchanged statement.