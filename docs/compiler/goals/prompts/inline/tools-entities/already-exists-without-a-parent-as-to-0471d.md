---
purpose: >-
  Instruction fragment: `{}` already exists without a parent as `{}`.
used_by:
  - "bootstrap/src/tools/entities.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
`{}` already exists without a parent as `{}`; to place it under `{}` call update_entity {{id: {}, parent: {}}}. A second `{}` under that parent is a different concept only with a `note` saying what tells them apart