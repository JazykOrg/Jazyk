---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Update an existing entity. A rename keeps the id. add_aliases records a wording, remove_aliases takes one back (a wording the documents turn out not to use). remove_parent: true takes the entity out of the containment tree (a generic rule-carrier that should stand beside no parent). attributes upsert by name; attributes not named stand. parent obeys the same gates as on create.