---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Update an existing requirement's statement, entities, edges, transition, or facets; a field given replaces the stored one whole. section plus quote (together) re-anchor the provenance; the quote must locate verbatim in the section. Omit both when only the other fields change.