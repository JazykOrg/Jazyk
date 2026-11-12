---
purpose: >-
  Instruction fragment: `quote` is the document's own sentence, never the statement.
used_by:
  - "bootstrap/src/tools/requirements.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
`quote` is the document's own sentence, never the statement. To change only the entities or edges, omit `section` and `quote`