---
purpose: >-
  Instruction fragment: Update an existing entity.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Update an existing entity. A rename keeps the id. add_aliases records a wording, remove_aliases takes one back (a wording the documents turn out not to use). remove_parent: true takes the entity out of the containment tree (a generic rule-carrier that should stand beside no parent). attributes upsert by name; attributes not named stand. parent obeys the same gates as on create.