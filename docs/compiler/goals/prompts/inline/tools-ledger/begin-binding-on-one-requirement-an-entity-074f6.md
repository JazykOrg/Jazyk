---
workflow: >-
  The agent records which generated files implement each documented requirement and how
  each requirement is tested. These tools save that accounting, called the generation
  and verification ledger. These messages explain what a valid recording must include.
used_by:
  - "bootstrap/src/tools/ledger.rs"
---
begin_binding on one requirement; an entity's generation task becomes useful once every row of that entity is bound, so bind an entity's rows together