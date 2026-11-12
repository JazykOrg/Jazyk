---
purpose: >-
  Instruction fragment: `{}` looks like a name variant of existing `{}` ({}).
used_by:
  - "bootstrap/src/tools/entities.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
`{}` looks like a name variant of existing `{}` ({}); if it is the same concept, reuse that id and add your wording with update_entity add_aliases. A field, part, state, or child concept of it IS a different concept: repeat the call with a `note` naming that relation