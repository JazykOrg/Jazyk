---
purpose: >-
  Instruction fragment: `{}` looks like a name variant of existing `{}` ({}), a sibling at
  this level that already holds children.
used_by:
  - "bootstrap/src/tools/entities.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
`{}` looks like a name variant of existing `{}` ({}), a sibling at this level that already holds children; a lookalike of an existing area reuses it: reparent the members under {} with update_entity parent, or pick the name the documents use for this area