---
workflow: >-
  The verification workflow selects tests, inspects their recorded evidence, and decides
  which ones can run in the current environment. These messages explain that selection
  or an execution restriction, including legacy commands that require a local native
  runner.
used_by:
  - "bootstrap/src/verify/select.rs"
---
all selected rows pass, but the executed suite has failed cases or an unsuccessful run; inspect suite.failures and run evidence before claiming whole-suite success. No owner is inferred for an unbound case