---
purpose: >-
  Instruction fragment: `{}` belongs to entity `{}`, outside your component group.
used_by:
  - "bootstrap/src/tools/files.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
`{}` belongs to entity `{}`, outside your component group; never write to another group's file. Reference it (import, include, link, read) and pick a path of your own; when it already carries one of your requirements, name it on that requirement's test row (`files: [path]`) in record_generation