---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Look up entities by name or alias (kind entity, the default) or views by title (kind view) before creating one. Pass query for one name, or queries for every name a section introduces in one call (one result per query, in order). Returns up to 8 possible matches per name; hits can be false neighbors (substrings, shared words), so inspect each before reusing it.