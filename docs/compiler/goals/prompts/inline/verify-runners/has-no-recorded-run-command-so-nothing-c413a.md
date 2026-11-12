---
purpose: >-
  Instruction fragment: `{}` has no recorded run command, so nothing can execute it.
used_by:
  - "bootstrap/src/verify/runners.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
`{}` has no recorded run command, so nothing can execute it; define a saved suite task and record its test declaration with record_binding, or record_generation for entity `{}`