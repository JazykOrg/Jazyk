---
purpose: >-
  Instruction fragment: `{}` is staged in this session and not committed yet: the loaded
  set renders the committed snapshot, the staged node is exactly what your write call
  sent, and it loads after done.
used_by:
  - "bootstrap/src/tools/read.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
`{}` is staged in this session and not committed yet: the loaded set renders the committed snapshot, the staged node is exactly what your write call sent, and it loads after done