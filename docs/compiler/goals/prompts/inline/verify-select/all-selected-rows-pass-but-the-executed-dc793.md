---
purpose: >-
  Instruction fragment: all selected rows pass, but the executed suite has failed cases or
  an unsuccessful run.
used_by:
  - "bootstrap/src/verify/select.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
all selected rows pass, but the executed suite has failed cases or an unsuccessful run; inspect suite.failures and run evidence before claiming whole-suite success. No owner is inferred for an unbound case